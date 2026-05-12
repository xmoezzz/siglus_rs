//! Desktop pump-mode FFI for macOS bundle launchers and other GUI hosts.
//!
//! A bundle UI can own its process/UI lifecycle and drive Siglus with
//! `siglus_pump_step` rather than entering winit's blocking `run_app`.

#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]

use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::time::Duration;

use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::host::{cstr_required, parse_bool_exit, SiglusHost, SiglusHostConfig, SiglusNativeMessageBoxCallback};
use crate::render::Renderer;
use crate::runtime::game_display_info::resolve_game_name_from_project_dir;
use crate::runtime::input::{VmKey, VmMouseButton};

pub struct SiglusPumpHandle {
    event_loop: EventLoop<()>,
    app: PumpApp,
}

struct PumpApp {
    config: SiglusHostConfig,
    window: Option<&'static Window>,
    window_id: Option<WindowId>,
    host: Option<SiglusHost>,
    init_error: Option<String>,
    exit_requested: bool,
    native_messagebox_callback: Option<SiglusNativeMessageBoxCallback>,
    native_messagebox_user_data: *mut c_void,
}

impl PumpApp {
    fn new(config: SiglusHostConfig) -> Self {
        Self {
            config,
            window: None,
            window_id: None,
            host: None,
            init_error: None,
            exit_requested: false,
            native_messagebox_callback: None,
            native_messagebox_user_data: std::ptr::null_mut(),
        }
    }

    fn ensure_created(&mut self, elwt: &ActiveEventLoop) {
        if self.window.is_some() || self.init_error.is_some() {
            return;
        }
        let width = self.config.width.unwrap_or(1280).max(1);
        let height = self.config.height.unwrap_or(720).max(1);
        let title = resolve_game_name_from_project_dir(&self.config.project_dir);
        let window = match elwt.create_window(
            WindowAttributes::default()
                .with_title(title)
                .with_inner_size(LogicalSize::new(width as f64, height as f64)),
        ) {
            Ok(w) => w,
            Err(e) => {
                self.init_error = Some(format!("create window: {e:?}"));
                elwt.exit();
                return;
            }
        };
        let window: &'static Window = Box::leak(Box::new(window));
        let renderer = match pollster::block_on(Renderer::new(window)) {
            Ok(r) => r,
            Err(e) => {
                self.init_error = Some(format!("renderer init: {e:?}"));
                elwt.exit();
                return;
            }
        };
        let mut host = match pollster::block_on(SiglusHost::new_with_renderer(self.config.clone(), renderer)) {
            Ok(h) => h,
            Err(e) => {
                self.init_error = Some(format!("host init: {e:?}"));
                elwt.exit();
                return;
            }
        };
        host.set_native_messagebox_callback(self.native_messagebox_callback, self.native_messagebox_user_data);
        self.window_id = Some(window.id());
        self.window = Some(window);
        self.host = Some(host);
        window.request_redraw();
    }

    fn handle_event(&mut self, event: Event<()>, elwt: &ActiveEventLoop) {
        match event {
            Event::Resumed => self.ensure_created(elwt),
            Event::WindowEvent { window_id, event } if self.window_id == Some(window_id) => {
                self.handle_window_event(event, elwt);
            }
            Event::AboutToWait => {
                if self.exit_requested {
                    elwt.exit();
                    return;
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn handle_window_event(&mut self, event: WindowEvent, elwt: &ActiveEventLoop) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                self.exit_requested = true;
                elwt.exit();
            }
            WindowEvent::Resized(size) => {
                let sf = self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0);
                host.resize(size.width.max(1), size.height.max(1), sf);
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { state: ElementState::Pressed, physical_key: PhysicalKey::Code(code), text, .. },
                ..
            } => {
                if let Some(k) = map_keycode(code) {
                    host.key_down(k);
                }
                if let Some(text) = text.as_ref() {
                    if text.chars().any(|c| !c.is_control()) {
                        host.text_input(text);
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { state: ElementState::Released, physical_key: PhysicalKey::Code(code), .. },
                ..
            } => {
                if let Some(k) = map_keycode(code) {
                    host.key_up(k);
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => host.text_input(&text),
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = if let Some(w) = self.window.as_ref() {
                    let p = position.to_logical::<f64>(w.scale_factor());
                    (p.x, p.y)
                } else {
                    (position.x, position.y)
                };
                host.mouse_move(x, y);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(b) = map_mouse_button(button) {
                    match state {
                        ElementState::Pressed => {
                            if matches!(b, VmMouseButton::Left) {
                                let (x, y) = current_mouse_pos(host);
                                host.touch(0, x, y);
                            } else {
                                host.mouse_down(b);
                            }
                        }
                        ElementState::Released => {
                            if matches!(b, VmMouseButton::Left) {
                                let (x, y) = current_mouse_pos(host);
                                host.touch(2, x, y);
                            } else {
                                host.mouse_up(b);
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 120.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y.round() as i32,
                };
                host.mouse_wheel(dy);
            }
            WindowEvent::RedrawRequested => {
                let status = parse_bool_exit(host.step(16), "siglus_pump_step/redraw");
                if status != 0 {
                    self.exit_requested = true;
                    elwt.exit();
                }
            }
            _ => {}
        }
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
        Enter => Some(VmKey::Enter),
        Space => Some(VmKey::Space),
        Backspace => Some(VmKey::Backspace),
        Tab => Some(VmKey::Tab),
        ShiftLeft | ShiftRight => Some(VmKey::Shift),
        AltLeft | AltRight => Some(VmKey::Alt),
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

#[no_mangle]
pub unsafe extern "C" fn siglus_pump_create(
    game_root_utf8: *const c_char,
    _nls_utf8: *const c_char,
) -> *mut SiglusPumpHandle {
    let game_root = match cstr_required(game_root_utf8, "game_root_utf8") {
        Ok(s) => s,
        Err(e) => {
            log::error!("siglus_pump_create: {e:?}");
            return std::ptr::null_mut();
        }
    };
    let config = SiglusHostConfig::new(PathBuf::from(game_root));
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            log::error!("siglus_pump_create: EventLoop::new: {e:?}");
            return std::ptr::null_mut();
        }
    };
    let mut handle = Box::new(SiglusPumpHandle { event_loop, app: PumpApp::new(config) });
    {
        let event_loop = &mut handle.event_loop;
        let app = &mut handle.app;
        let _ = event_loop.pump_events(Some(Duration::from_millis(0)), |event, elwt| {
            app.handle_event(event, elwt);
        });
    }
    Box::into_raw(handle)
}

#[no_mangle]
pub unsafe extern "C" fn siglus_pump_set_native_messagebox_callback(
    handle: *mut SiglusPumpHandle,
    callback: Option<SiglusNativeMessageBoxCallback>,
    user_data: *mut c_void,
) {
    if handle.is_null() {
        return;
    }
    let h = &mut *handle;
    h.app.native_messagebox_callback = callback;
    h.app.native_messagebox_user_data = user_data;
    if let Some(host) = h.app.host.as_mut() {
        host.set_native_messagebox_callback(callback, user_data);
    }
}

#[no_mangle]
pub unsafe extern "C" fn siglus_pump_submit_messagebox_result(
    handle: *mut SiglusPumpHandle,
    request_id: u64,
    value: i64,
) {
    if handle.is_null() {
        return;
    }
    let h = &mut *handle;
    if let Some(host) = h.app.host.as_mut() {
        host.submit_native_messagebox_result(request_id, value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn siglus_pump_step(handle: *mut SiglusPumpHandle, timeout_ms: u32) -> i32 {
    if handle.is_null() {
        return 2;
    }
    let h = &mut *handle;
    if let Some(w) = h.app.window.as_ref() {
        w.request_redraw();
    }
    let status = {
        let event_loop = &mut h.event_loop;
        let app = &mut h.app;
        event_loop.pump_events(Some(Duration::from_millis(timeout_ms.max(1) as u64)), |event, elwt| {
            app.handle_event(event, elwt);
        })
    };
    match status {
        PumpStatus::Continue => 0,
        _ => 1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn siglus_pump_destroy(handle: *mut SiglusPumpHandle) {
    if handle.is_null() {
        return;
    }
    drop(Box::from_raw(handle));
}

#[no_mangle]
pub unsafe extern "C" fn siglus_run_entry(game_root_utf8: *const c_char, nls_utf8: *const c_char) -> i32 {
    let handle = siglus_pump_create(game_root_utf8, nls_utf8);
    if handle.is_null() {
        return 1;
    }
    loop {
        let r = siglus_pump_step(handle, 16);
        if r != 0 {
            siglus_pump_destroy(handle);
            return 0;
        }
    }
}
