//! Desktop pump-mode FFI for macOS bundle launchers and other GUI hosts.
//!
//! A bundle UI can own its process/UI lifecycle and drive Siglus with
//! `siglus_pump_step` rather than entering winit's blocking `run_app`.

#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]

use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::time::Duration;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::event_loop::run_on_demand::EventLoopExtRunOnDemand;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::monitor::Fullscreen;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::desktop_viewport::DesktopViewport;
use crate::host::{
    SiglusHost, SiglusHostConfig, SiglusNativeMessageBoxCallback, cstr_opt, cstr_required,
    parse_bool_exit,
};
use crate::render::Renderer;
use crate::runtime::CommandContext;
use crate::runtime::game_display_info::resolve_game_name_from_project_dir;
use crate::runtime::input::{VmKey, VmMouseButton};

pub struct SiglusPumpHandle {
    event_loop: EventLoop,
    app: PumpApp,
}

struct PumpApp {
    config: SiglusHostConfig,
    window: Option<&'static dyn Window>,
    window_id: Option<WindowId>,
    host: Option<SiglusHost>,
    init_error: Option<String>,
    exit_requested: bool,
    native_messagebox_callback: Option<SiglusNativeMessageBoxCallback>,
    native_messagebox_user_data: *mut c_void,
    /// Size the window and map input in physical pixels, ignoring the
    /// display's HiDPI scale (the launcher app does this for every engine).
    physical_pixels: bool,
    window_mode: WindowModeState,
}

/// Keep native window observations separate from script/config requests.
#[derive(Default)]
struct WindowModeState {
    last_requested: Option<(bool, i64)>,
    last_observed: Option<bool>,
    pending_fullscreen: Option<bool>,
    requested_scale: Option<u32>,
}

impl WindowModeState {
    fn observe(&mut self, ctx: &mut CommandContext, fullscreen: bool) {
        use crate::runtime::forms::codes::syscom_op::GET_WINDOW_MODE;
        let previous = self.last_observed.replace(fullscreen);
        if self.pending_fullscreen == Some(fullscreen) {
            self.pending_fullscreen = None;
            return;
        }
        if self.pending_fullscreen.is_some()
            || previous.is_none()
            || previous == Some(fullscreen)
            || self.last_requested.is_none()
            || ctx.globals.syscom.window_config_dirty != 0
        {
            return;
        }
        // A system fullscreen button is an observation, not another request.
        // Reflect it in GET_WINDOW_MODE and CONFIG without fighting it next frame.
        let mode = i64::from(fullscreen);
        ctx.globals.syscom.config_int.insert(GET_WINDOW_MODE, mode);
        ctx.globals.syscom.original_config.screen_size_mode = mode;
        if let Some(last) = self.last_requested.as_mut() {
            last.0 = fullscreen;
        }
    }

    fn update(&mut self, ctx: &mut CommandContext) -> Option<bool> {
        use crate::runtime::forms::codes::syscom_op::{GET_WINDOW_MODE, GET_WINDOW_MODE_SIZE};
        use crate::runtime::forms::syscom::{WINDOW_MODE_REQUEST, WINDOW_SIZE_REQUEST};
        let st = &mut ctx.globals.syscom;
        let fullscreen = st
            .config_int
            .get(&GET_WINDOW_MODE)
            .copied()
            .unwrap_or(st.original_config.screen_size_mode)
            != 0;
        let scale = st
            .config_int
            .get(&GET_WINDOW_MODE_SIZE)
            .copied()
            .unwrap_or(st.original_config.screen_size_scale.0);
        let dirty = std::mem::take(&mut st.window_config_dirty);
        let previous = self.last_requested.replace((fullscreen, scale));
        if fullscreen {
            self.requested_scale = None;
        }
        if !fullscreen
            && (previous.is_none_or(|old| old.0 || old.1 != scale)
                || dirty & WINDOW_SIZE_REQUEST != 0)
        {
            self.requested_scale = Some(scale.clamp(25, 400) as u32);
        }
        if previous.map(|old| old.0) != Some(fullscreen) || dirty & WINDOW_MODE_REQUEST != 0 {
            self.pending_fullscreen =
                (self.last_observed != Some(fullscreen)).then_some(fullscreen);
            Some(fullscreen)
        } else {
            None
        }
    }

    fn take_window_scale(&mut self) -> Option<u32> {
        self.requested_scale.take()
    }
}

impl PumpApp {
    fn new(config: SiglusHostConfig) -> Self {
        Self {
            physical_pixels: false,
            config,
            window: None,
            window_id: None,
            host: None,
            init_error: None,
            exit_requested: false,
            native_messagebox_callback: None,
            native_messagebox_user_data: std::ptr::null_mut(),
            window_mode: WindowModeState::default(),
        }
    }

    /// The window's scale factor, or 1 in physical-pixel mode.
    fn scale_factor(&self) -> f64 {
        if self.physical_pixels {
            return 1.0;
        }
        self.window.map_or(1.0, |w| w.scale_factor())
    }

    fn ensure_created(&mut self, elwt: &dyn ActiveEventLoop) {
        if self.window.is_some() || self.init_error.is_some() {
            return;
        }
        let width = self.config.width.unwrap_or(1280).max(1);
        let height = self.config.height.unwrap_or(720).max(1);
        let title = resolve_game_name_from_project_dir(&self.config.project_dir);
        let attributes = WindowAttributes::default().with_title(title);
        let attributes = if self.physical_pixels {
            attributes.with_surface_size(winit::dpi::PhysicalSize::new(width, height))
        } else {
            attributes.with_surface_size(LogicalSize::new(width as f64, height as f64))
        };
        let window = match elwt.create_window(attributes) {
            Ok(w) => w,
            Err(e) => {
                self.init_error = Some(format!("create window: {e:?}"));
                elwt.exit();
                return;
            }
        };
        let window: &'static dyn Window = Box::leak(window);
        let renderer = match pollster::block_on(Renderer::new(window)) {
            Ok(r) => r,
            Err(e) => {
                self.init_error = Some(format!("renderer init: {e:?}"));
                elwt.exit();
                return;
            }
        };
        let mut host = match pollster::block_on(SiglusHost::new_with_renderer(
            self.config.clone(),
            renderer,
        )) {
            Ok(h) => h,
            Err(e) => {
                self.init_error = Some(format!("host init: {e:?}"));
                elwt.exit();
                return;
            }
        };
        host.set_native_messagebox_callback(
            self.native_messagebox_callback,
            self.native_messagebox_user_data,
        );
        // Preserve Gameexe's canvas size; resizing the window must only change
        // presentation, never the fixed coordinates used by game scripts.
        let size = window.surface_size();
        let scale = if self.physical_pixels {
            1.0
        } else {
            window.scale_factor() as f32
        };
        resize_game_viewport(&mut host, size.width, size.height, scale);
        self.window_id = Some(window.id());
        self.window = Some(window);
        self.host = Some(host);
        window.request_redraw();
    }

    fn handle_window_event(&mut self, event: WindowEvent, elwt: &dyn ActiveEventLoop) {
        let scale = self.scale_factor();
        let surface = self.window.map(|w| w.surface_size());
        let Some(host) = self.host.as_mut() else {
            return;
        };
        let (gw, gh) = host.logical_size();
        let viewport = surface.map(|s| DesktopViewport::fit(s.width, s.height, gw, gh));
        match event {
            WindowEvent::CloseRequested => {
                self.exit_requested = true;
                elwt.exit();
            }
            WindowEvent::SurfaceResized(size) => {
                let sf = scale as f32;
                resize_game_viewport(host, size.width, size.height, sf);
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(code),
                        text,
                        ..
                    },
                ..
            } => {
                if let Some(k) = map_keycode(code) {
                    host.key_down(k);
                }
                if host.vm_mut().ctx.editbox_accepts_direct_text()
                    && let Some(text) = text.as_deref()
                {
                    host.text_input(text);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        physical_key: PhysicalKey::Code(code),
                        ..
                    },
                ..
            } => {
                if let Some(k) = map_keycode(code) {
                    host.key_up(k);
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => host.ime_preedit(&text, cursor),
            WindowEvent::Ime(Ime::Commit(text)) => host.text_input(&text),
            WindowEvent::Ime(Ime::Disabled) => host.ime_disabled(),
            WindowEvent::Ime(Ime::Enabled) => {}
            WindowEvent::PointerMoved {
                position,
                primary: true,
                ..
            }
            | WindowEvent::PointerEntered {
                position,
                primary: true,
                ..
            } => {
                let Some(viewport) = viewport else {
                    return;
                };
                let (x, y) = viewport.game_position(position.x, position.y);
                host.mouse_move(x, y);
            }
            WindowEvent::PointerButton {
                state,
                button,
                position,
                primary: true,
                ..
            } => {
                let Some(button) = button.mouse_button() else {
                    return;
                };
                let Some(viewport) = viewport else {
                    return;
                };
                let (x, y) = viewport.game_position(position.x, position.y);
                host.mouse_move(x, y);
                if let Some(b) = map_mouse_button(button) {
                    match (state, b) {
                        (ElementState::Pressed, VmMouseButton::Left) => {
                            let (x, y) = current_mouse_pos(host);
                            host.touch(0, x, y);
                        }
                        (ElementState::Released, VmMouseButton::Left) => {
                            let (x, y) = current_mouse_pos(host);
                            host.touch(2, x, y);
                        }
                        (ElementState::Pressed, b) => host.mouse_down(b),
                        (ElementState::Released, b) => host.mouse_up(b),
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 120.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y.round() as i32,
                    _ => 0,
                };
                host.mouse_wheel(dy);
            }
            WindowEvent::RedrawRequested => {
                if let Some(window) = self.window.as_ref() {
                    self.window_mode
                        .observe(&mut host.vm_mut().ctx, window.fullscreen().is_some());
                    apply_ime_window_state(*window, host);
                }
                let status = parse_bool_exit(host.step(16), "siglus_pump_step/redraw");
                if status != 0 {
                    self.exit_requested = true;
                    elwt.exit();
                } else if let Some(window) = self.window.as_ref() {
                    if let Some(fullscreen) = self.window_mode.update(&mut host.vm_mut().ctx) {
                        window.set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
                    }
                    // Cocoa fullscreen transitions are asynchronous. Keep the
                    // size request until the native window has left fullscreen.
                    if window.fullscreen().is_none()
                        && let Some(percent) = self.window_mode.take_window_scale()
                    {
                        let (gw, gh) = host.logical_size();
                        let size = winit::dpi::PhysicalSize::new(
                            (gw.saturating_mul(percent) / 100).max(1),
                            (gh.saturating_mul(percent) / 100).max(1),
                        );
                        if let Some(size) = window.request_surface_size(size.into()) {
                            // Wayland may apply the resize without another event.
                            resize_game_viewport(host, size.width, size.height, scale as f32);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for PumpApp {
    fn can_create_surfaces(&mut self, elwt: &dyn ActiveEventLoop) {
        self.ensure_created(elwt);
    }

    fn window_event(
        &mut self,
        elwt: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window_id == Some(window_id) {
            self.handle_window_event(event, elwt);
        }
    }

    fn about_to_wait(&mut self, elwt: &dyn ActiveEventLoop) {
        if self.exit_requested {
            elwt.exit();
            return;
        }
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
        // elwt.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(16)));
        elwt.set_control_flow(ControlFlow::Poll);
    }
}

fn resize_game_viewport(host: &mut SiglusHost, width: u32, height: u32, scale: f32) {
    let (gw, gh) = host.logical_size();
    let (x, y, w, h) = DesktopViewport::fit(width, height, gw, gh).rect();
    host.resize_with_logical_viewport(width.max(1), height.max(1), scale, gw, gh, x, y, w, h);
}

fn apply_ime_window_state(window: &dyn Window, host: &mut SiglusHost) {
    if let Some((x, y, width, height)) = host.vm_mut().ctx.focused_editbox_ime_area() {
        let surface = window.surface_size();
        let (gw, gh) = host.logical_size();
        let viewport = DesktopViewport::fit(surface.width, surface.height, gw, gh);
        let (px, py) = viewport.surface_position(x.max(0) as f64, y.max(0) as f64);
        let (pw, ph) = viewport.surface_extent(width.max(1) as f64, height.max(1) as f64);
        let position =
            winit::dpi::PhysicalPosition::new(px.round() as i32, py.round() as i32).into();
        let size =
            winit::dpi::PhysicalSize::new(pw.round().max(1.0) as u32, ph.round().max(1.0) as u32)
                .into();
        crate::ime::enable_ime(window, position, size);
    } else {
        crate::ime::disable_ime(window);
    }
}

fn current_mouse_pos(host: &mut SiglusHost) -> (f64, f64) {
    let input = &host.vm_mut().ctx.input;
    (input.mouse_x as f64, input.mouse_y as f64)
}

fn map_mouse_button(b: MouseButton) -> Option<VmMouseButton> {
    match b {
        MouseButton::Left => Some(VmMouseButton::Left),
        MouseButton::Right => Some(VmMouseButton::Right),
        MouseButton::Middle => Some(VmMouseButton::Middle),
        _ => None,
    }
}

fn map_keycode(k: KeyCode) -> Option<VmKey> {
    use KeyCode::*;
    match k {
        Escape => Some(VmKey::Escape),
        Enter | NumpadEnter => Some(VmKey::Enter),
        Space => Some(VmKey::Space),
        Backspace => Some(VmKey::Backspace),
        Delete => Some(VmKey::Delete),
        Tab => Some(VmKey::Tab),
        ShiftLeft | ShiftRight => Some(VmKey::Shift),
        ControlLeft | ControlRight => Some(VmKey::Control),
        MetaLeft | MetaRight => Some(VmKey::Meta),
        AltLeft | AltRight => Some(VmKey::Alt),
        Home => Some(VmKey::Home),
        End => Some(VmKey::End),
        ArrowLeft => Some(VmKey::ArrowLeft),
        ArrowUp => Some(VmKey::ArrowUp),
        ArrowRight => Some(VmKey::ArrowRight),
        ArrowDown => Some(VmKey::ArrowDown),
        KeyA => Some(VmKey::Letter('A')),
        KeyB => Some(VmKey::Letter('B')),
        KeyC => Some(VmKey::Letter('C')),
        KeyD => Some(VmKey::Letter('D')),
        KeyE => Some(VmKey::Letter('E')),
        KeyF => Some(VmKey::Letter('F')),
        KeyG => Some(VmKey::Letter('G')),
        KeyH => Some(VmKey::Letter('H')),
        KeyI => Some(VmKey::Letter('I')),
        KeyJ => Some(VmKey::Letter('J')),
        KeyK => Some(VmKey::Letter('K')),
        KeyL => Some(VmKey::Letter('L')),
        KeyM => Some(VmKey::Letter('M')),
        KeyN => Some(VmKey::Letter('N')),
        KeyO => Some(VmKey::Letter('O')),
        KeyP => Some(VmKey::Letter('P')),
        KeyQ => Some(VmKey::Letter('Q')),
        KeyR => Some(VmKey::Letter('R')),
        KeyS => Some(VmKey::Letter('S')),
        KeyT => Some(VmKey::Letter('T')),
        KeyU => Some(VmKey::Letter('U')),
        KeyV => Some(VmKey::Letter('V')),
        KeyW => Some(VmKey::Letter('W')),
        KeyX => Some(VmKey::Letter('X')),
        KeyY => Some(VmKey::Letter('Y')),
        KeyZ => Some(VmKey::Letter('Z')),
        Digit0 => Some(VmKey::Digit(0)),
        Digit1 => Some(VmKey::Digit(1)),
        Digit2 => Some(VmKey::Digit(2)),
        Digit3 => Some(VmKey::Digit(3)),
        Digit4 => Some(VmKey::Digit(4)),
        Digit5 => Some(VmKey::Digit(5)),
        Digit6 => Some(VmKey::Digit(6)),
        Digit7 => Some(VmKey::Digit(7)),
        Digit8 => Some(VmKey::Digit(8)),
        Digit9 => Some(VmKey::Digit(9)),
        F1 => Some(VmKey::F(1)),
        F2 => Some(VmKey::F(2)),
        F3 => Some(VmKey::F(3)),
        F4 => Some(VmKey::F(4)),
        F5 => Some(VmKey::F(5)),
        F6 => Some(VmKey::F(6)),
        F7 => Some(VmKey::F(7)),
        F8 => Some(VmKey::F(8)),
        F9 => Some(VmKey::F(9)),
        F10 => Some(VmKey::F(10)),
        F11 => Some(VmKey::F(11)),
        F12 => Some(VmKey::F(12)),
        _ => None,
    }
}

/// Create a host to drive with `siglus_pump_step`, or return null on failure.
///
/// # Safety
///
/// If non-null, `game_root_utf8` must point to a NUL-terminated string in a
/// single readable allocation of at most `isize::MAX` bytes, including the
/// terminator. The bytes must remain valid and unchanged for this call.
/// Call on the platform event-loop thread (the main thread on platforms that
/// require it). Use and destroy the returned handle only on that thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_create(
    game_root_utf8: *const c_char,
) -> *mut SiglusPumpHandle {
    let game_root = match unsafe { cstr_required(game_root_utf8, "game_root_utf8") } {
        Ok(s) => s,
        Err(e) => {
            log::error!("siglus_pump_create: {e:?}");
            return std::ptr::null_mut();
        }
    };
    let config = SiglusHostConfig::new(PathBuf::from(game_root));
    let mut event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            log::error!("siglus_pump_create: EventLoop::new: {e:?}");
            return std::ptr::null_mut();
        }
    };
    let mut handle = Box::new(SiglusPumpHandle {
        event_loop,
        app: PumpApp::new(config),
    });
    {
        let event_loop = &mut handle.event_loop;
        let app = &mut handle.app;
        let _ = event_loop.pump_app_events(Some(Duration::from_millis(0)), app);
    }
    Box::into_raw(handle)
}

/// Set the callback used to display native message boxes.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
/// The callback and any data it accesses through `user_data` must remain valid
/// until the callback is replaced or the handle is destroyed. The callback
/// must not unwind or reenter functions that access this handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_set_native_messagebox_callback(
    handle: *mut SiglusPumpHandle,
    callback: Option<SiglusNativeMessageBoxCallback>,
    user_data: *mut c_void,
) {
    if handle.is_null() {
        return;
    }
    let h = unsafe { &mut *handle };
    h.app.native_messagebox_callback = callback;
    h.app.native_messagebox_user_data = user_data;
    if let Some(host) = h.app.host.as_mut() {
        host.set_native_messagebox_callback(callback, user_data);
    }
}

/// Submit the result of a native message box.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_submit_messagebox_result(
    handle: *mut SiglusPumpHandle,
    request_id: u64,
    value: i64,
) {
    if handle.is_null() {
        return;
    }
    let h = unsafe { &mut *handle };
    if let Some(host) = h.app.host.as_mut() {
        host.submit_native_messagebox_result(request_id, value);
    }
}

/// Send committed text to the host.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
/// If non-null, `text_utf8` must point to a NUL-terminated string in a single
/// readable allocation of at most `isize::MAX` bytes, including the terminator.
/// The bytes must remain valid and unchanged for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_text_input(
    handle: *mut SiglusPumpHandle,
    text_utf8: *const c_char,
) {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return;
    };
    let Some(host) = handle.app.host.as_mut() else {
        return;
    };
    if let Some(text) = unsafe { cstr_opt(text_utf8) } {
        host.text_input(&text);
    }
}

/// Update IME composition, or disable IME if the text pointer is null.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
/// If non-null, `text_utf8` must point to a NUL-terminated string in a single
/// readable allocation of at most `isize::MAX` bytes, including the terminator.
/// The bytes must remain valid and unchanged for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_ime_preedit(
    handle: *mut SiglusPumpHandle,
    text_utf8: *const c_char,
    cursor_start: i32,
    cursor_end: i32,
) {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return;
    };
    let Some(host) = handle.app.host.as_mut() else {
        return;
    };
    if text_utf8.is_null() {
        host.ime_disabled();
        return;
    }
    let text = unsafe { CStr::from_ptr(text_utf8) }
        .to_string_lossy()
        .into_owned();
    let cursor = if cursor_start >= 0 && cursor_end >= 0 {
        Some((cursor_start as usize, cursor_end as usize))
    } else {
        None
    };
    host.ime_preedit(&text, cursor);
}

/// Send a key press to the host.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_key_down(handle: *mut SiglusPumpHandle, key_code: i32) {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return;
    };
    let Some(host) = handle.app.host.as_mut() else {
        return;
    };
    host.key_down_code(key_code);
}

/// Send a key release to the host.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_key_up(handle: *mut SiglusPumpHandle, key_code: i32) {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return;
    };
    let Some(host) = handle.app.host.as_mut() else {
        return;
    };
    host.key_up_code(key_code);
}

/// Pump pending events and return the host status.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_step(handle: *mut SiglusPumpHandle, timeout_ms: u32) -> i32 {
    if handle.is_null() {
        return 2;
    }
    let h = unsafe { &mut *handle };
    if let Some(w) = h.app.window.as_ref() {
        w.request_redraw();
    }
    let status = {
        let event_loop = &mut h.event_loop;
        let app = &mut h.app;
        event_loop.pump_app_events(Some(Duration::from_millis(timeout_ms.max(1) as u64)), app)
    };
    match status {
        PumpStatus::Continue => 0,
        _ => 1,
    }
}

/// Destroy a pump host. A null pointer is ignored.
///
/// # Safety
///
/// If non-null, `handle` must be a live pointer returned by `siglus_pump_create`.
/// Call on the thread that created it, with exclusive access to the handle
/// for the entire call, including during callbacks.
/// The handle must not have been destroyed before. It and all pointers into it
/// become invalid after this call and must not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_pump_destroy(handle: *mut SiglusPumpHandle) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle) });
}

/// Run the host event loop until exit.
///
/// # Safety
///
/// If non-null, `game_root_utf8` must point to a NUL-terminated string in a
/// single readable allocation of at most `isize::MAX` bytes, including the
/// terminator. The bytes must remain valid and unchanged for this call.
/// Call on the platform event-loop thread (the main thread on platforms that
/// require it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_run_entry(game_root_utf8: *const c_char) -> i32 {
    let game_root = match unsafe { cstr_required(game_root_utf8, "game_root_utf8") } {
        Ok(s) => s,
        Err(e) => {
            log::error!("siglus_run_entry: {e:?}");
            return 1;
        }
    };
    run_game(&game_root, false)
}

/// Runs a game in its own window until it ends. With `physical_pixels`, the
/// window is sized in physical pixels and input is mapped without the
/// display's HiDPI scale (what the launcher app uses for every engine).
pub fn run_game(game_root: &str, physical_pixels: bool) -> i32 {
    let mut event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            log::error!("siglus_run_entry: EventLoop::new: {e:?}");
            return 1;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let config = SiglusHostConfig::new(PathBuf::from(game_root));
    let mut app = PumpApp::new(config);
    app.physical_pixels = physical_pixels;
    match event_loop.run_app_on_demand(&mut app) {
        Ok(()) => {
            if let Some(e) = app.init_error {
                log::error!("siglus_run_entry: {e}");
                1
            } else {
                0
            }
        }
        Err(e) => {
            log::error!("siglus_run_entry: run_app: {e:?}");
            1
        }
    }
}

#[cfg(test)]
mod window_mode_tests {
    use super::*;
    use crate::runtime::CommandContext;
    use crate::runtime::forms::codes::syscom_op::GET_WINDOW_MODE;
    use crate::runtime::forms::syscom;

    #[test]
    fn windowed_startup_is_applied_only_once() {
        let mut ctx = CommandContext::new(PathBuf::from("."));
        let mut mode = WindowModeState::default();
        assert_eq!(mode.update(&mut ctx), Some(false));
        assert_eq!(mode.update(&mut ctx), None);
    }

    #[test]
    fn config_dialog_switches_fullscreen_on_and_off() {
        let mut ctx = CommandContext::new(PathBuf::from("."));
        let mut mode = WindowModeState::default();
        assert_eq!(mode.update(&mut ctx), Some(false));
        let mut config = syscom::config_state_for_save(&ctx);
        config.screen_size_mode = 1;
        syscom::apply_config_dialog_state(&mut ctx, config.clone());
        assert_eq!(mode.update(&mut ctx), Some(true));
        assert_eq!(mode.update(&mut ctx), None);
        config.screen_size_mode = 0;
        syscom::apply_config_dialog_state(&mut ctx, config);
        assert_eq!(mode.update(&mut ctx), Some(false));
    }

    #[test]
    fn restored_fullscreen_mode_is_honored_without_a_runtime_override() {
        let mut ctx = CommandContext::new(PathBuf::from("."));
        ctx.globals.syscom.original_config.screen_size_mode = 1;
        ctx.globals.syscom.config_int.remove(&GET_WINDOW_MODE);
        let mut mode = WindowModeState::default();
        assert_eq!(mode.update(&mut ctx), Some(true));
    }

    #[test]
    fn script_setting_overrides_restored_fullscreen_mode() {
        let mut ctx = CommandContext::new(PathBuf::from("."));
        ctx.globals.syscom.original_config.screen_size_mode = 1;
        ctx.globals.syscom.config_int.insert(GET_WINDOW_MODE, 0);
        let mut mode = WindowModeState::default();
        assert_eq!(mode.update(&mut ctx), Some(false));
    }
}
