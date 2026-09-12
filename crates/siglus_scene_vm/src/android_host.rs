//! Android host-driven Siglus FFI.
//!
//! Android owns the UI/event loop and the `ANativeWindow`.  The platform calls
//! these `siglus_android_*` functions to create, step, resize, deliver touch input,
//! and destroy the engine instance.

#![cfg(target_os = "android")]

use std::ffi::{c_char, c_void, CStr};
use std::ptr::NonNull;
use std::sync::Once;

use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle};

use crate::host::{cstr_opt, default_frame_interval_ms, parse_bool_exit, SiglusHost, SiglusHostConfig, SiglusNativeMessageBoxCallback};
use crate::render::Renderer;

static ANDROID_CTX_ONCE: Once = Once::new();

static ANDROID_PANIC_HOOK: Once = Once::new();

fn install_android_panic_hook() {
    ANDROID_PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let location = info
                .location()
                .map(|loc| format!("{}:{}", loc.file(), loc.line()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            log::error!("[SIGLUS_ANDROID_PANIC] panic at {location}: {message}");
            log::error!(
                "[SIGLUS_ANDROID_PANIC] backtrace:\n{}",
                std::backtrace::Backtrace::force_capture()
            );
        }));
    });
}

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
        // Default to Warn: at Debug the frame loop emits ~950 lines/s on device
        // (91% of them Debug), and formatting + one logcat write per line dominated
        // the frame budget -- the app sat at ~106% CPU and felt permanently laggy.
        // Every diagnostic we actually grep for is log::warn!, so Warn keeps them.
        // Override with SIGLUS_LOG=<level> when a Debug-level trace is really needed.
        let level = std::env::var("SIGLUS_LOG")
            .ok()
            .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
                "off" => Some(log::LevelFilter::Off),
                "error" => Some(log::LevelFilter::Error),
                "warn" => Some(log::LevelFilter::Warn),
                "info" => Some(log::LevelFilter::Info),
                "debug" => Some(log::LevelFilter::Debug),
                "trace" => Some(log::LevelFilter::Trace),
                _ => None,
            })
            .unwrap_or(log::LevelFilter::Warn);
        let _ = android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(level)
                .with_tag("siglus_rs"),
        );
        log::info!("siglus_android_init_context: ndk_context initialized");
        install_android_panic_hook();
    });
}

fn aspect_fit_viewport(surface_w: u32, surface_h: u32, logical_w: u32, logical_h: u32) -> (u32, u32, u32, u32) {
    let sw = surface_w.max(1) as f64;
    let sh = surface_h.max(1) as f64;
    let lw = logical_w.max(1) as f64;
    let lh = logical_h.max(1) as f64;
    let scale = (sw / lw).min(sh / lh);
    let vw = (lw * scale).round().max(1.0).min(sw) as u32;
    let vh = (lh * scale).round().max(1.0).min(sh) as u32;
    let vx = ((surface_w.max(1).saturating_sub(vw)) / 2) as u32;
    let vy = ((surface_h.max(1).saturating_sub(vh)) / 2) as u32;
    (vx, vy, vw, vh)
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
    let mut host = pollster::block_on(SiglusHost::new_with_renderer(config, renderer))?;
    let (logical_w, logical_h) = host.logical_size();
    let (vx, vy, vw, vh) = aspect_fit_viewport(width_px, height_px, logical_w, logical_h);
    log::info!(
        "siglus_android_create: surface={}x{} scale={:.3} logical={}x{} viewport={}x{}+{}+{}",
        width_px,
        height_px,
        native_scale_factor,
        logical_w,
        logical_h,
        vw,
        vh,
        vx,
        vy
    );
    host.resize_with_logical_viewport(
        width_px,
        height_px,
        native_scale_factor as f32,
        logical_w,
        logical_h,
        vx,
        vy,
        vw,
        vh,
    );
    Ok(Box::new(host))
}

#[no_mangle]
pub unsafe extern "C" fn siglus_android_create(
    native_window_ptr: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
    native_scale_factor: f64,
    game_dir_utf8: *const c_char,
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
    let (logical_w, logical_h) = host.logical_size();
    let (vx, vy, vw, vh) = aspect_fit_viewport(surface_width_px, surface_height_px, logical_w, logical_h);
    host.resize_with_logical_viewport(
        surface_width_px.max(1),
        surface_height_px.max(1),
        sf,
        logical_w,
        logical_h,
        vx,
        vy,
        vw,
        vh,
    );
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
    // Map physical SurfaceView pixels into the game's logical screen through
    // the aspect-fit viewport (VM input uses logical game-window coordinates).
    let (vx, vy, vw, vh) = host.renderer_mut().surface_viewport();
    let (lw, lh) = host.logical_size();
    let vm_x = ((x_px - vx as f64) / vw.max(1) as f64 * lw as f64).clamp(0.0, lw as f64);
    let vm_y = ((y_px - vy as f64) / vh.max(1) as f64 * lh as f64).clamp(0.0, lh as f64);
    if crate::host::sg_input_trace() {
        log::warn!(
            "[SG_INPUT_DEBUG] touch phase={} px=({:.1},{:.1}) viewport=({},{} {}x{}) logical={}x{} vm=({:.1},{:.1})",
            phase, x_px, y_px, vx, vy, vw, vh, lw, lh, vm_x, vm_y
        );
    }
    host.touch(phase, vm_x, vm_y);
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
pub unsafe extern "C" fn siglus_android_ime_preedit(
    handle: *mut c_void,
    text_utf8: *const c_char,
    cursor_start: i32,
    cursor_end: i32,
) {
    let Some(host) = (handle as *mut SiglusHost).as_mut() else {
        return;
    };
    if text_utf8.is_null() {
        host.ime_disabled();
        return;
    }
    let text = CStr::from_ptr(text_utf8).to_string_lossy().into_owned();
    let cursor = if cursor_start >= 0 && cursor_end >= 0 {
        Some((cursor_start as usize, cursor_end as usize))
    } else {
        None
    };
    host.ime_preedit(&text, cursor);
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
