//! Android host-driven Siglus FFI.
//!
//! Android owns the UI/event loop and the `ANativeWindow`.  The platform calls
//! these `siglus_android_*` functions to create, step, resize, deliver touch input,
//! and destroy the engine instance.

#![cfg(target_os = "android")]

use std::ffi::{c_char, c_void};
use std::ptr::NonNull;
use std::sync::Once;

use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle};

use crate::host::{cstr_opt, default_frame_interval_ms, parse_bool_exit, SiglusHost, SiglusHostConfig, SiglusNativeMessageBoxCallback};
use crate::render::Renderer;

static ANDROID_CTX_ONCE: Once = Once::new();

#[no_mangle]
pub unsafe extern "C" fn siglus_android_init_context(java_vm_ptr: *mut c_void, context_ptr: *mut c_void) {
    if java_vm_ptr.is_null() || context_ptr.is_null() {
        log::error!("siglus_android_init_context: null java_vm_ptr/context_ptr");
        return;
    }
    ANDROID_CTX_ONCE.call_once(|| {
        unsafe {
            ndk_context::initialize_android_context(java_vm_ptr, context_ptr);
        }
        log::info!("siglus_android_init_context: ndk_context initialized");
    });
}

unsafe fn build_host(
    native_window_ptr: *mut c_void,
    width_px: u32,
    height_px: u32,
    native_scale_factor: f64,
    game_dir_utf8: *const c_char,
) -> anyhow::Result<Box<SiglusHost>> {
    let native_window = NonNull::new(native_window_ptr)
        .ok_or_else(|| anyhow::anyhow!("native_window_ptr is null"))?;
    let game_dir = cstr_opt(game_dir_utf8)
        .ok_or_else(|| anyhow::anyhow!("game_dir is null or empty"))?;

    let raw_display_handle = RawDisplayHandle::Android(AndroidDisplayHandle::new());
    let raw_window_handle = RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(native_window));
    let scale = if native_scale_factor.is_finite() && native_scale_factor > 0.0 {
        native_scale_factor as f32
    } else {
        1.0
    };
    let renderer = pollster::block_on(Renderer::new_from_raw_handles(
        raw_display_handle,
        raw_window_handle,
        width_px.max(1),
        height_px.max(1),
        scale,
    ))?;
    let mut config = SiglusHostConfig::new(std::path::PathBuf::from(game_dir));
    config.width = Some(((width_px as f32) / scale).max(1.0).round() as u32);
    config.height = Some(((height_px as f32) / scale).max(1.0).round() as u32);
    pollster::block_on(SiglusHost::new_with_renderer(config, renderer)).map(Box::new)
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_create(
    native_window_ptr: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
    native_scale_factor: f64,
    game_dir_utf8: *const c_char,
    _nls_utf8: *const c_char,
) -> *mut c_void {
    match build_host(native_window_ptr, surface_width_px, surface_height_px, native_scale_factor, game_dir_utf8) {
        Ok(host) => Box::into_raw(host) as *mut c_void,
        Err(e) => {
            log::error!("siglus_android_create: {e:?}");
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_set_native_messagebox_callback(
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
pub unsafe extern "C" fn siglus_android_submit_messagebox_result(
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
pub unsafe extern "C" fn siglus_android_step(handle: *mut c_void, dt_ms: u32) -> i32 {
    if handle.is_null() {
        return 1;
    }
    let host = &mut *(handle as *mut SiglusHost);
    parse_bool_exit(host.step(default_frame_interval_ms(dt_ms)), "siglus_android_step")
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_resize(
    handle: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
) {
    if handle.is_null() {
        return;
    }
    let host = &mut *(handle as *mut SiglusHost);
    let sf = host.renderer_mut().scale_factor();
    host.resize(surface_width_px.max(1), surface_height_px.max(1), sf);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_set_surface(
    handle: *mut c_void,
    native_window_ptr: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
) {
    // WGPU surface replacement is not exposed by the current renderer.  The safe
    // host contract is to destroy and recreate when Android gives us a different
    // ANativeWindow.  Keep this function as a no-op ABI hook so old Java-side
    // lifecycle code can call it without corrupting renderer state.
    let _ = (handle, native_window_ptr, surface_width_px, surface_height_px);
    log::warn!("siglus_android_set_surface: recreate engine instance for a new ANativeWindow");
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_touch(
    handle: *mut c_void,
    phase: i32,
    x_px: f64,
    y_px: f64,
) {
    if handle.is_null() {
        return;
    }
    let host = &mut *(handle as *mut SiglusHost);
    // The VM input model uses logical game-window coordinates, while Android
    // delivers physical pixel positions from SurfaceView.
    let sf = host.renderer_mut().scale_factor() as f64;
    host.touch(phase, x_px / sf.max(1.0), y_px / sf.max(1.0));
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_text_input(handle: *mut c_void, text_utf8: *const c_char) {
    let Some(host) = (handle as *mut SiglusHost).as_mut() else {
        return;
    };
    if let Some(text) = cstr_opt(text_utf8) {
        host.text_input(&text);
    }
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_key_down(handle: *mut c_void, key_code: i32) {
    let Some(host) = (handle as *mut SiglusHost).as_mut() else {
        return;
    };
    host.key_down_code(key_code);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_key_up(handle: *mut c_void, key_code: i32) {
    let Some(host) = (handle as *mut SiglusHost).as_mut() else {
        return;
    };
    host.key_up_code(key_code);
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_destroy(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    drop(Box::from_raw(handle as *mut SiglusHost));
}

#[no_mangle]
pub unsafe extern "C" fn android_main(_app: *mut c_void) {}
