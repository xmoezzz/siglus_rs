//! Packed element-code helpers.
//!
//! Original macro:
//! `((owner << 24) + (group << 16) + code)`.

pub const ELM_OWNER_USER_PROP: u8 = crate::runtime::forms::codes::ELM_OWNER_USER_PROP as u8;
pub const ELM_OWNER_USER_CMD: u8 = crate::runtime::forms::codes::ELM_OWNER_USER_CMD as u8;
pub const ELM_OWNER_CALL_PROP: u8 = crate::runtime::forms::codes::ELM_OWNER_CALL_PROP as u8;
pub const ELM_OWNER_CALL_CMD: u8 = crate::runtime::forms::codes::ELM_OWNER_CALL_CMD as u8;
pub const ELM_OWNER_FUNCTION: u8 = crate::runtime::forms::codes::ELM_OWNER_FUNCTION as u8;
pub const ELM_OWNER_FORM: u8 = crate::runtime::forms::codes::ELM_OWNER_FORM as u8;

#[inline]
pub const fn create(owner: i32, group: i32, code: i32) -> i32 {
    crate::runtime::forms::codes::create_elm_code(owner, group, code)
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
pub fn code(x: i32) -> u16 {
    code16(x)
}
