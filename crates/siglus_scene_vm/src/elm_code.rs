//! Packed element-code helpers.
//!
//! The Siglus VM frequently encodes command/property identifiers into a
//! single 32-bit "element code":
//!
//! ```text
//! owner (8 bits) | group (8 bits) | code (16 bits)
//! ```
//!
//! We keep this module tiny and dependency-free so the bring-up can proceed
//! even when the full constant table is not available.

/// Owner values observed in Siglus headers.
///
/// These are *not* game-specific constants; they are part of the engine.
pub const ELM_OWNER_USER_PROP: u8 = 127;
pub const ELM_OWNER_USER_CMD: u8 = 126;
pub const ELM_OWNER_CALL_PROP: u8 = 125;
pub const ELM_OWNER_CALL_CMD: u8 = 124;
pub const ELM_OWNER_FUNCTION: u8 = 123;

/// Legacy (non-packed) element owner.
///
/// Many VM "forms" are represented as small integers (e.g. 70, 135) whose
/// packed owner byte is zero.
pub const ELM_OWNER_FORM: u8 = 0;

/// Mask used by the original engine to classify packed elements by
/// owner/group bucket in multiple dispatch paths (`code & 0xFFF00000`).
pub const ELM_BUCKET_MASK: u32 = 0xFFF0_0000;

/// Recovered high-level packed element buckets observed directly in the
/// original executable's decompiled dispatchers.
///
/// These are not full per-element IDs yet; they are the stable owner/group
/// families that the engine routes on before examining low bits.
pub mod bucket {
    pub const B100: u32 = 0x1000_0000;
    pub const B101: u32 = 0x1010_0000;
    pub const B103: u32 = 0x1030_0000;
    pub const B104: u32 = 0x1040_0000;
    pub const B105: u32 = 0x1050_0000;
    pub const B106: u32 = 0x1060_0000;
    pub const B107: u32 = 0x1070_0000;
    pub const B10D: u32 = 0x10D0_0000;
    pub const B10E: u32 = 0x10E0_0000;
    pub const B10F: u32 = 0x10F0_0000;
    pub const B110: u32 = 0x1100_0000;
    pub const B114: u32 = 0x1140_0000;

    pub const B200: u32 = 0x2000_0000;
    pub const B201: u32 = 0x2010_0000;
    pub const B202: u32 = 0x2020_0000;
    pub const B203: u32 = 0x2030_0000;
    pub const B204: u32 = 0x2040_0000;
    pub const B205: u32 = 0x2050_0000;
    pub const B207: u32 = 0x2070_0000;
    pub const B208: u32 = 0x2080_0000;
    pub const B209: u32 = 0x2090_0000;

    pub const B300: u32 = 0x3000_0000;

    pub const B500: u32 = 0x5000_0000;
    pub const B501: u32 = 0x5010_0000;
    pub const B503: u32 = 0x5030_0000;

    pub const B600: u32 = 0x6000_0000;
    pub const B604: u32 = 0x6040_0000;
    pub const B607: u32 = 0x6070_0000;
    pub const B608: u32 = 0x6080_0000;
    pub const B609: u32 = 0x6090_0000;
    pub const B60A: u32 = 0x60A0_0000;
    pub const B60D: u32 = 0x60D0_0000;
    pub const B60E: u32 = 0x60E0_0000;
    pub const B60F: u32 = 0x60F0_0000;
    pub const B610: u32 = 0x6100_0000;
    pub const B611: u32 = 0x6110_0000;
    pub const B612: u32 = 0x6120_0000;
    pub const B613: u32 = 0x6130_0000;

    pub const B700: u32 = 0x7000_0000;
    pub const B701: u32 = 0x7010_0000;
    pub const B702: u32 = 0x7020_0000;
    pub const B703: u32 = 0x7030_0000;
    pub const B704: u32 = 0x7040_0000;
    pub const B705: u32 = 0x7050_0000;
    pub const B707: u32 = 0x7070_0000;
    pub const B708: u32 = 0x7080_0000;
    pub const B709: u32 = 0x7090_0000;
    pub const B70B: u32 = 0x70B0_0000;
    pub const B70C: u32 = 0x70C0_0000;
    pub const B70D: u32 = 0x70D0_0000;
    pub const B730: u32 = 0x7300_0000;
    pub const B731: u32 = 0x7310_0000;
    pub const B732: u32 = 0x7320_0000;
    pub const B740: u32 = 0x7400_0000;
    pub const B741: u32 = 0x7410_0000;
    pub const B742: u32 = 0x7420_0000;
    pub const B744: u32 = 0x7440_0000;
    pub const B745: u32 = 0x7450_0000;
    pub const B746: u32 = 0x7460_0000;
    pub const B747: u32 = 0x7470_0000;
}

#[inline]
pub fn is_packed_element(x: i32) -> bool {
    ((x as u32) >> 24) != 0
}

#[inline]
pub fn owner(x: i32) -> u8 {
    ((x as u32) >> 24) as u8
}

#[inline]
pub fn group(x: i32) -> u8 {
    (((x as u32) >> 16) & 0xFF) as u8
}

#[inline]
pub fn code16(x: i32) -> u16 {
    ((x as u32) & 0xFFFF) as u16
}

#[inline]
pub fn bucket(x: i32) -> u32 {
    (x as u32) & ELM_BUCKET_MASK
}

/// Convenience alias used across the VM/runtime for the packed 16-bit code.
#[inline]
pub fn code(x: i32) -> u16 {
    code16(x)
}
