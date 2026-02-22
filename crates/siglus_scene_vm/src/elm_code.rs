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

/// Convenience alias used across the VM/runtime for the packed 16-bit code.
#[inline]
pub fn code(x: i32) -> u16 {
    code16(x)
}
