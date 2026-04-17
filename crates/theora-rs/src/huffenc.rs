use crate::bitpack::PackBuf;
use crate::codec::{HuffCode, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES};
use crate::error::{Result, TheoraError};
use crate::huffman::OC_NDCT_TOKEN_BITS;
use crate::packet::PackWriter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HuffEntry {
    pattern: u32,
    shift: i32,
    token: usize,
}

pub const TH_VP31_HUFF_CODES: [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES] = [
    [
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0026,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0166,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x004E,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02CE,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x059E,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x027D,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x04F9,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00B2,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x04F8,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x059F,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x009E,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x013F,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0058,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0047,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01FF,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x008C,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x03FC,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x046A,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0469,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x11A1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x007D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x007E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x011B,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x08D1,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x03FD,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x046B,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x11A0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x007C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00FE,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0086,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0087,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0367,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x06CC,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x06CB,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x006E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x366D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x006F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x006D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0364,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0D9A,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x06CA,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1B37,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x366C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0042,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00D8,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00F7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0058,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0167,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x02CB,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x02CA,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x1661,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0078,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x007A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0164,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0599,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x02CD,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0B31,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x1660,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0079,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F6,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x007A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0072,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0399,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0030,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00E7,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x01CD,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0398,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x007B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00EC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00D1,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x01A6,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0D21,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01A7,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01A5,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0349,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x00D0,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0691,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0D20,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00ED,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00B7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x016C,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0099,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x005A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x16D8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x004D,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x2DB3,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x02DA,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x05B7,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0098,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0B6D,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x2DB2,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0027,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0056,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00AF,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x148A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0050,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00AE,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x2917,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0290,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0523,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0149,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0A44,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x2916,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0053,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00A5,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00F5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00F4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x024D,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0499,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0498,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x007B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0048,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0092,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0127,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0040,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0061,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0060,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00F5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0041,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x007B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0131,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0261,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0260,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0027,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x004D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0099,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F9,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x00F8,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0050,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x007D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0053,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0295,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0294,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x00A4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x007C,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x006A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00D7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x007D,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x014B,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0068,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00D6,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x007F,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02F0,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0198,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0179,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0064,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0067,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00CD,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x007E,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02F1,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0065,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00BD,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0199,
            nbits: 9,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x015D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x015C,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x02BF,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x006A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0064,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00CB,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00AD,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02BE,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x006E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x006F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x006B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00CA,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00AC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x015E,
            nbits: 9,
        },
    ],
    [
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00D6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0551,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0AA1,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0057,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x006A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0068,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0056,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00E5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0155,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0AA0,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0073,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00D7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00AB,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00E4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00A9,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0151,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0150,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x02A9,
            nbits: 10,
        },
    ],
    [
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x017A,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x02F7,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0BDB,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x17B4,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x2F6B,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x2F6A,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00BC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x005C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x05EC,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x004B,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00C6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x031D,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0C71,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0C70,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0C73,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0030,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0062,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x004A,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x018F,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0C72,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x008D,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0040,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0239,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0471,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x08E0,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x11C3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0041,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x008F,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x008C,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x011D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x11C2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0043,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0050,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0042,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0173,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x02E4,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x172D,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x172C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x05CA,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00B8,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0B97,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00B9,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02E3,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x05C4,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x1715,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x005E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0170,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1714,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0B8B,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0079,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00F0,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0119,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0230,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x08C4,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x008D,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x118B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x118A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0047,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F1,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0463,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0046,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01E3,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x03C5,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x1E21,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0079,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F0,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x1E20,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1E23,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1E22,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0032,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0047,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0789,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0061,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x004E,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x009E,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x027C,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x09F5,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x09F4,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0060,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0026,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x013F,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x04FB,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00AB,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00AA,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0119,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0461,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0460,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0047,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x008D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0231,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0054,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x004D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0131,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0261,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0027,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0099,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0260,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0095,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0251,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x04A0,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00C8,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0065,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x004B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00C9,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0129,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0943,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0942,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0395,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0E51,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00E4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0073,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01CB,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0729,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1CA1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1CA0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01F5,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x07D1,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01F6,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0032,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F8,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00F9,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01F7,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x03E9,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0FA0,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x1F43,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1F42,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0177,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0176,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00BA,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0061,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00C1,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0180,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0302,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0607,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0606,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0076,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0078,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0393,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0073,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00E5,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x01C8,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0E4A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1C97,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1C96,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0E49,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E48,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0079,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0054,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01B7,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x06D9,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0DB1,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0DB0,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x00AB,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00AA,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x006C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00DA,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x036D,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00B6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x006A,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x05B9,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x16E1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x16E0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x016F,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x006B,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0B71,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x02DD,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x007F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00A1,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00A0,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x020C,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0834,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x106B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0082,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0040,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x041B,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x106A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x007E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0107,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00F6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00E9,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x03A1,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0740,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0E82,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x01EF,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01D1,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1D07,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D06,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x007A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01EE,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0042,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00D9,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0362,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x06C6,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0086,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0087,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01B0,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0D8F,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0D8E,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x006D,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0076,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x014D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0533,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x14C9,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00A5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00A7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0298,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x14C8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x14CB,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x14CA,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00A4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00ED,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0283,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0A0A,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00A1,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00A3,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00A2,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0140,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1417,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1416,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0A09,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0A08,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00EC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00E4,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01CF,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0072,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0073,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01CE,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x039B,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0398,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0733,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0732,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0735,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0734,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00E5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0050,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0172,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x02E7,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1732,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x2E67,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2E66,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00B8,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x05CD,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1731,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1730,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0040,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00AD,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02B0,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1589,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1588,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0057,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0041,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0159,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0563,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x158B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x158A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0046,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0045,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0064,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x032A,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0657,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0047,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0044,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00CB,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0656,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0329,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0328,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0023,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0194,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1956,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x32AF,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0076,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0064,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0197,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0196,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x032B,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0654,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x32AE,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1955,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1954,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0048,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x093F,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x24FA,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0032,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0067,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0066,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0092,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0126,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x024E,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x049E,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x49F7,
            nbits: 15,
        },
        HuffCode {
            pattern: 0x49F6,
            nbits: 15,
        },
        HuffCode {
            pattern: 0x24F9,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x24F8,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00EA,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1D75,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0066,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01D6,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x03AF,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1D74,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D77,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D76,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0EB9,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0EB8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0032,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0067,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00E6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0684,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0072,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00E7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00D1,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01A0,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0686,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0D0F,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0D0A,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x1A17,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1A16,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1A1D,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1A1C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00BC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0053,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0050,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00A4,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0294,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x052B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052D,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052F,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052E,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00BD,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00D0,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01A3,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0344,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0D14,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x1A2B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x068B,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1A2A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0032,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0030,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00C5,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0621,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0620,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0026,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0027,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0063,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0189,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0623,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0622,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0023,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0043,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0044,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0114,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0455,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0023,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0042,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x008B,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0454,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0457,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0456,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00A0,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0285,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1425,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0051,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0143,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0508,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1424,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1427,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1426,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0053,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x014A,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0037,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00A4,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x052D,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052F,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x052E,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01F3,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x07C7,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01F0,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x007D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01F2,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x07C6,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x07C5,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1F12,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x3E27,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E26,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1F11,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1F10,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x004F,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x04E5,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0026,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x009D,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x04E4,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x04E7,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x04E6,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x04E1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x04E0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x04E3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x04E2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0032,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00E8,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x1D2A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0E96,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x3A57,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3A56,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3A5D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3A5C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3A5F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3A5E,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D29,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D28,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0076,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01D3,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x03A4,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00BF,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0085,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0211,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0842,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x1087,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0043,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00BE,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0109,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1086,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0841,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0840,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x006F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0061,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0374,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1BA8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x3753,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0060,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00DC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01BB,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x06EB,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1BAB,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x3752,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3755,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3754,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0025,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0024,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01DA,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1DBD,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x3B7C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0077,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00EC,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x03B6,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x076E,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1DBF,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x76FB,
            nbits: 15,
        },
        HuffCode {
            pattern: 0x76FA,
            nbits: 15,
        },
        HuffCode {
            pattern: 0x3B79,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B78,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x008F,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0475,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x11D1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0079,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0027,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0026,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0046,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x011C,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0477,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x08ED,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x11D0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x11D3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x11D2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x11D9,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x11D8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0022,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0078,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x03E2,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x3E3D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0034,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x007D,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01F0,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x07C6,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x3E3C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E3F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E3E,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E39,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E38,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E3B,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3E3A,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0035,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00F9,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00E0,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0E1E,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0071,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01C2,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1C3F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1C3E,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0E19,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E18,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E1B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E1A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E1D,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E1C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0043,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0042,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x016C,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x16D1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x02DF,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x016E,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x02DE,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x16D0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x16D3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x16D2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x2DB5,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2DB4,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2DB7,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2DB6,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x16D9,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x16D8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x005A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x05B7,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x05B5,
            nbits: 11,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01AC,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1AD8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x35B3,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B2,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0068,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x35BD,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BC,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BF,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BE,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B9,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B8,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BB,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BA,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B5,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B4,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x01A9,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x01A8,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x035A,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x00D7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00D5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x35B7,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B6,
            nbits: 14,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0072,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0071,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0154,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0AAB,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0AA8,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0070,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0073,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0054,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00AB,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02AB,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1553,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1552,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1555,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1554,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0029,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0345,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x006B,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x006A,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0344,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0347,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0346,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x01A1,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x01A0,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0012,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0013,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0056,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x015C,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x15D5,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00AF,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x02BB,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x15D4,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x15D7,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x15D6,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x15D1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x15D0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x15D3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x15D2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0030,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0106,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x006E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x006F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x020F,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x020E,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0101,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0100,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0103,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0102,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0105,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0104,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 7,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x006E,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00AD,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x00AF,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0014,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002A,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0576,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0AEF,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0AEE,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0571,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0570,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0573,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0572,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0575,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0574,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0036,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x006F,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00AC,
            nbits: 10,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0526,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1495,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x00A6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x1494,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1497,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1496,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1491,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1490,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1493,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1492,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x293D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x293C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x293F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00A5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0148,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00A7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0A4E,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x293E,
            nbits: 14,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0526,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1495,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x00A6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x1494,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1497,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1496,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1491,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1490,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1493,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1492,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x293D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x293C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x293F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00A5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0148,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00A7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0A4E,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x293E,
            nbits: 14,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x002F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0526,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1495,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x00A6,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x002D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x1494,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1497,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1496,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1491,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1490,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1493,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1492,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x293D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x293C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x293F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0028,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00A5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0148,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00A7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x002E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0015,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0A4E,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x293E,
            nbits: 14,
        },
    ],
    [
        HuffCode {
            pattern: 0x0003,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x010D,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0863,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x0860,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0075,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0042,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x010F,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x010E,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0219,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x10C3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x10C2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x10C5,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x10C4,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00E2,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x1C6F,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x38D9,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0008,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0070,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01C7,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x038C,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x071A,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x38D8,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x38DB,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x38DA,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x38DD,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x38DC,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001D,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003D,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000B,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001E,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0074,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x03AA,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1D5C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0021,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x00EB,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x01D4,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0EAF,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x3ABB,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3ABA,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D59,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D58,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D5B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D5A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x003F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0020,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 6,
        },
    ],
    [
        HuffCode {
            pattern: 0x0004,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x016A,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x16B1,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x005B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x02D7,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0B5A,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x16B0,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x16B3,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x16B2,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x2D6D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2D6C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2D6F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x2D6E,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000A,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x002C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0017,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0016,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x00B4,
            nbits: 8,
        },
    ],
    [
        HuffCode {
            pattern: 0x0005,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000D,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0005,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0009,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0033,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0193,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x192C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0061,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0031,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0010,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0011,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x00C8,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x192F,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x325B,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x325A,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1929,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1928,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x192B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x192A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x325D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x325C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0018,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x001A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0065,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0019,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0004,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0007,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0060,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x0324,
            nbits: 10,
        },
    ],
    [
        HuffCode {
            pattern: 0x0006,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x0039,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01D9,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1D82,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0761,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x03BE,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x000E,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0762,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x3B07,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B06,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B1D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B1C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B1F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B1E,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B19,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B18,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x3B1B,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0038,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01DE,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00ED,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x03BF,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x00EE,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0EC0,
            nbits: 12,
        },
        HuffCode {
            pattern: 0x3B1A,
            nbits: 14,
        },
    ],
    [
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 3,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x0006,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01D0,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x0E8C,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D1B,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D1A,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0003,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0002,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x00EA,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x00E9,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x0E89,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E88,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E8B,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x0E8A,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x1D65,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D64,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D67,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D66,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D61,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D60,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x03AD,
            nbits: 11,
        },
        HuffCode {
            pattern: 0x1D63,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D62,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D1D,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D1C,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x01D7,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x1D1F,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x1D1E,
            nbits: 14,
        },
    ],
    [
        HuffCode {
            pattern: 0x0002,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x000F,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x001C,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x000C,
            nbits: 4,
        },
        HuffCode {
            pattern: 0x003B,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x01AC,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x1AD8,
            nbits: 13,
        },
        HuffCode {
            pattern: 0x35B3,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B2,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x0001,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0000,
            nbits: 2,
        },
        HuffCode {
            pattern: 0x0069,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x0068,
            nbits: 7,
        },
        HuffCode {
            pattern: 0x35BD,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BC,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BF,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BE,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B9,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B8,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BB,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35BA,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B5,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B4,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x01A9,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x01A8,
            nbits: 9,
        },
        HuffCode {
            pattern: 0x035A,
            nbits: 10,
        },
        HuffCode {
            pattern: 0x00D7,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x00D5,
            nbits: 8,
        },
        HuffCode {
            pattern: 0x003A,
            nbits: 6,
        },
        HuffCode {
            pattern: 0x001B,
            nbits: 5,
        },
        HuffCode {
            pattern: 0x35B7,
            nbits: 14,
        },
        HuffCode {
            pattern: 0x35B6,
            nbits: 14,
        },
    ],
];

fn huff_entry_cmp(a: &HuffEntry, b: &HuffEntry) -> core::cmp::Ordering {
    a.pattern.cmp(&b.pattern)
}

pub fn oc_huff_codes_pack(
    writer: &mut PackWriter,
    codes: &[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
) -> Result<()> {
    for table in codes.iter() {
        let maxlen = table.iter().map(|c| c.nbits).max().unwrap_or(0);
        if maxlen <= 0 || maxlen > 32 {
            return Err(TheoraError::InvalidArgument);
        }
        let mask: u32 = (((1u64 << (maxlen as u32)) - 1) & 0xFFFF_FFFF) as u32;
        let mut entries = [HuffEntry {
            pattern: 0,
            shift: 0,
            token: 0,
        }; TH_NDCT_TOKENS];
        for (j, code) in table.iter().enumerate() {
            let shift = maxlen - code.nbits;
            entries[j] = HuffEntry {
                shift,
                pattern: (code.pattern << shift) & mask,
                token: j,
            };
        }
        entries.sort_by(huff_entry_cmp);
        let mut bpos = maxlen;
        for j in 0..TH_NDCT_TOKENS {
            if entries[j].shift >= maxlen {
                return Err(TheoraError::InvalidArgument);
            }
            while bpos > entries[j].shift {
                writer.write(0, 1);
                bpos -= 1;
            }
            writer.write(1, 1);
            writer.write(entries[j].token as u32, OC_NDCT_TOKEN_BITS as usize);
            let mut bit = 1u32.checked_shl(bpos as u32).unwrap_or(0);
            while bit != 0 && (entries[j].pattern & bit) != 0 {
                bpos += 1;
                bit = bit.checked_shl(1).unwrap_or(0);
            }
            if j + 1 < TH_NDCT_TOKENS {
                if bit == 0 {
                    return Err(TheoraError::InvalidArgument);
                }
                let prefix_mask = (!((bit - 1) as u32)).wrapping_shl(1);
                if (entries[j + 1].pattern & bit) == 0
                    || (entries[j].pattern & prefix_mask) != (entries[j + 1].pattern & prefix_mask)
                {
                    return Err(TheoraError::InvalidArgument);
                }
            } else if bpos < maxlen {
                return Err(TheoraError::InvalidArgument);
            }
        }
    }
    Ok(())
}

pub fn oc_huff_codes_unpack(
    opb: &mut PackBuf<'_>,
) -> Result<[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES]> {
    let mut codes = [[HuffCode::default(); TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES];
    for table in &mut codes {
        let mut code: u32 = 0;
        let mut len: i32 = 0;
        let mut nleaves = 0;
        *table = [HuffCode::default(); TH_NDCT_TOKENS];
        loop {
            let bits = opb.read1() as i32;
            if opb.bytes_left() < 0 {
                return Err(TheoraError::BadHeader);
            }
            if bits == 0 {
                len += 1;
                if len > 32 {
                    return Err(TheoraError::BadHeader);
                }
            } else {
                nleaves += 1;
                if nleaves > TH_NDCT_TOKENS as i32 {
                    return Err(TheoraError::BadHeader);
                }
                let token = opb.read(OC_NDCT_TOKEN_BITS) as usize;
                if table[token].nbits > 0 {
                    return Err(TheoraError::InvalidArgument);
                }
                table[token].pattern = if len == 0 {
                    0
                } else {
                    code >> (32 - len as u32)
                };
                table[token].nbits = len;
                let mut code_bit = if len > 0 {
                    0x8000_0000u32 >> (len as u32 - 1)
                } else {
                    0
                };
                while len > 0 && (code & code_bit) != 0 {
                    code ^= code_bit;
                    code_bit = code_bit.checked_shl(1).unwrap_or(0);
                    len -= 1;
                }
                if len <= 0 {
                    break;
                }
                code |= code_bit;
            }
        }
        if nleaves < TH_NDCT_TOKENS as i32 {
            return Err(TheoraError::InvalidArgument);
        }
    }
    Ok(codes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_default_codes_round_trip_shape() {
        let mut writer = PackWriter::new();
        oc_huff_codes_pack(&mut writer, &TH_VP31_HUFF_CODES).unwrap();
        let buf = writer.finish();
        let mut pb = PackBuf::new(&buf);
        let unpacked = oc_huff_codes_unpack(&mut pb).unwrap();
        assert_eq!(unpacked[0][0].pattern, TH_VP31_HUFF_CODES[0][0].pattern);
        assert_eq!(unpacked[79][31].nbits, TH_VP31_HUFF_CODES[79][31].nbits);
    }
}
