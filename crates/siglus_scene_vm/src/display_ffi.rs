//! C ABI helpers for bundle/mobile launchers that need Siglus display metadata.

use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;

use crate::runtime::game_display_info::{
    resolve_game_cover_from_project_dir, resolve_game_name_from_project_dir,
};

unsafe fn path_from_cstr(ptr: *const c_char) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn into_c_string_ptr(s: String) -> *mut c_char {
    CString::new(s)
        .unwrap_or_else(|_| CString::new("Siglus").unwrap())
        .into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn siglus_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(CString::from_raw(ptr));
}

#[no_mangle]
pub unsafe extern "C" fn siglus_game_name_from_dir(game_root_utf8: *const c_char) -> *mut c_char {
    let Some(path) = path_from_cstr(game_root_utf8) else {
        return into_c_string_ptr("Siglus".to_string());
    };
    into_c_string_ptr(resolve_game_name_from_project_dir(path))
}

#[no_mangle]
pub unsafe extern "C" fn siglus_game_cover_path_from_dir(game_root_utf8: *const c_char) -> *mut c_char {
    let Some(path) = path_from_cstr(game_root_utf8) else {
        return std::ptr::null_mut();
    };
    let Some(cover) = resolve_game_cover_from_project_dir(path) else {
        return std::ptr::null_mut();
    };
    into_c_string_ptr(cover.source_path.to_string_lossy().to_string())
}

#[no_mangle]
pub unsafe extern "C" fn siglus_game_cover_mime_from_dir(game_root_utf8: *const c_char) -> *mut c_char {
    let Some(path) = path_from_cstr(game_root_utf8) else {
        return std::ptr::null_mut();
    };
    let Some(cover) = resolve_game_cover_from_project_dir(path) else {
        return std::ptr::null_mut();
    };
    into_c_string_ptr(cover.mime)
}
