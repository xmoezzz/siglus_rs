//! iOS host-driven Siglus FFI.
//!
//! UIKit/SwiftUI owns the platform run loop.  The host supplies a CAMetalLayer-backed
//! UIView pointer and advances the engine once per display-link tick.

#![cfg(target_os = "ios")]

use std::ffi::{c_char, c_void};
use std::ptr::NonNull;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, UiKitDisplayHandle, UiKitWindowHandle};

use crate::host::{cstr_required, default_frame_interval_ms, parse_bool_exit, SiglusHost, SiglusHostConfig, SiglusNativeMessageBoxCallback};
use crate::render::Renderer;

unsafe fn build_host(
    ui_view: *mut c_void,
    surface_width: u32,
    surface_height: u32,
    native_scale_factor: f64,
    game_root_utf8: *const c_char,
) -> anyhow::Result<Box<SiglusHost>> {
    let view = NonNull::new(ui_view).ok_or_else(|| anyhow::anyhow!("ui_view is null"))?;
    let game_root = cstr_required(game_root_utf8, "game_root_utf8")?;
    let scale = if native_scale_factor.is_finite() && native_scale_factor > 0.0 {
        native_scale_factor as f32
    } else {
        1.0
    };
    let raw_display_handle = RawDisplayHandle::UiKit(UiKitDisplayHandle::new());
    let raw_window_handle = RawWindowHandle::UiKit(UiKitWindowHandle::new(view));
    let renderer = pollster::block_on(Renderer::new_from_raw_handles(
        raw_display_handle,
        raw_window_handle,
        surface_width.max(1),
        surface_height.max(1),
        scale,
    ))?;
    let mut config = SiglusHostConfig::new(std::path::PathBuf::from(game_root));
    config.width = Some(((surface_width as f32) / scale).max(1.0).round() as u32);
    config.height = Some(((surface_height as f32) / scale).max(1.0).round() as u32);
    pollster::block_on(SiglusHost::new_with_renderer(config, renderer)).map(Box::new)
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_create(
    ui_view: *mut c_void,
    surface_width: u32,
    surface_height: u32,
    native_scale_factor: f64,
    game_root_utf8: *const c_char,
    _nls_utf8: *const c_char,
) -> *mut c_void {
    match build_host(ui_view, surface_width, surface_height, native_scale_factor, game_root_utf8) {
        Ok(host) => Box::into_raw(host) as *mut c_void,
        Err(e) => {
            log::error!("siglus_ios_create: {e:?}");
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_set_native_messagebox_callback(
    handle: *mut c_void,
    callback: Option<SiglusNativeMessageBoxCallback>,
    user_data: *mut c_void,
) {
    if handle.is_null() {
        return;
    }
    let host = &mut *(handle as *mut SiglusHost);
    host.set_native_messagebox_callback(callback, user_data);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_submit_messagebox_result(
    handle: *mut c_void,
    request_id: u64,
    value: i64,
) {
    if handle.is_null() {
        return;
    }
    let host = &mut *(handle as *mut SiglusHost);
    host.submit_native_messagebox_result(request_id, value);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_step(handle: *mut c_void, dt_ms: u32) -> i32 {
    if handle.is_null() {
        return 2;
    }
    let host = &mut *(handle as *mut SiglusHost);
    parse_bool_exit(host.step(default_frame_interval_ms(dt_ms)), "siglus_ios_step")
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_resize(
    handle: *mut c_void,
    surface_width: u32,
    surface_height: u32,
) {
    if handle.is_null() {
        return;
    }
    let host = &mut *(handle as *mut SiglusHost);
    let sf = host.renderer_mut().scale_factor();
    host.resize(surface_width.max(1), surface_height.max(1), sf);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_touch(
    handle: *mut c_void,
    phase: i32,
    x_points: f64,
    y_points: f64,
) {
    if handle.is_null() {
        return;
    }
    let host = &mut *(handle as *mut SiglusHost);
    // UIKit delivers points.  The VM input model is logical coordinates, so pass
    // points directly.
    host.touch(phase, x_points, y_points);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_ios_destroy(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    drop(Box::from_raw(handle as *mut SiglusHost));
}
