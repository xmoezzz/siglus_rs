//! Horizon host entry point for the existing Siglus VM.
//!
//! Unlike the desktop host this module owns libnx lifecycle/input.  It is kept
//! separate so VM, script and resource semantics stay shared with every other
//! target.

use anyhow::Result;
use std::ffi::{CStr, c_char};
use std::io::Write;
use std::path::PathBuf;

use crate::host::{SiglusHost, SiglusHostConfig};
use crate::render::Renderer;
use crate::runtime::input::{VmKey, VmMouseButton};

unsafe extern "C" {
    fn siglus_switch_random_fill(buffer: *mut u8, length: usize);
    fn siglus_switch_log_message(message: *const c_char);
    fn siglus_switch_set_error(message: *const c_char);
}

/// Write a NUL-terminated static startup marker through the C shell. This
/// avoids Rust stdio while the Horizon engine is still being constructed.
pub(crate) fn report_switch_marker(message: &'static [u8]) {
    debug_assert!(message.last() == Some(&0));
    unsafe { siglus_switch_log_message(message.as_ptr().cast()) };
}

/// Write a temporary runtime diagnostic assembled by the Horizon host. Keep
/// this here so shared VM code never gains a platform logging dependency.
pub(crate) fn report_switch_diagnostic(message: &str) {
    if let Ok(message_c) = std::ffi::CString::new(message) {
        unsafe { siglus_switch_log_message(message_c.as_ptr()) };
    }
}

fn report_switch_error(context: &str, err: &anyhow::Error) {
    let message = format!("{context}: {err:#}\n");
    eprintln!("{message}");
    if let Ok(message_c) = std::ffi::CString::new(message.as_str()) {
        unsafe {
            siglus_switch_set_error(message_c.as_ptr());
            siglus_switch_log_message(message_c.as_ptr());
        }
    }
    // Standard error is not normally visible from a homebrew launcher. Keep
    // a small append-only diagnostic on the mounted SD card without making
    // logging itself a startup requirement.
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("sdmc:/switch/siglus_rs/siglus_switch.log")
    {
        let _ = log.write_all(message.as_bytes());
    }
}

/// Entropy hook required by rand 0.9/eluna on Horizon. It is defined once in
/// this application root and delegates to libnx's kernel-seeded random source.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom_03::Error> {
    if dest.is_null() && len != 0 {
        return Err(getrandom_03::Error::UNEXPECTED);
    }
    unsafe { siglus_switch_random_fill(dest, len) };
    Ok(())
}

pub struct SwitchHost {
    host: SiglusHost,
    frame: u64,
    /// Frames to save as PNGs (`sdmc:/switch/siglus_rs/dump-frames`, one
    /// frame number per line; a diagnosis aid).
    dump_frames: Vec<u64>,
    /// The global save as last written (`global_save_fingerprint`).
    global_fingerprint: u64,
    held_buttons: u32,
    consumed_compatibility_keys: u32,
    pointer: Option<(f64, f64)>,
    pointer_left_down: bool,
    touch_active: bool,
}

/// How often the global save is checked for changes. Nothing else writes
/// it before the game's own end/return-to-menu, and closing the app from
/// HOME never gets there: the game would start as on its first boot again.
const GLOBAL_SAVE_INTERVAL: u64 = 300;

const DUMP_DIR: &str = "sdmc:/switch/siglus_rs/dump";

impl SwitchHost {
    pub fn new(config: SiglusHostConfig, width: u32, height: u32) -> Result<Self> {
        if crate::resource::project_needs_exe_key(&config.project_dir)? {
            anyhow::bail!(
                "This encrypted game needs key.toml in {}. Start the game once with the desktop player, then copy its key.toml here. Automatic key recovery is unavailable on Switch.",
                config.project_dir.display()
            );
        }
        report_switch_marker(b"siglus_switch: rust renderer-create begin\n\0");
        let renderer = Renderer::new(width, height)?;
        report_switch_marker(b"siglus_switch: rust renderer-create complete\n\0");
        report_switch_marker(b"siglus_switch: rust host-create begin\n\0");
        let mut host = SiglusHost::new_with_renderer_sync(config, renderer)?;
        report_switch_marker(b"siglus_switch: rust host-create complete\n\0");
        // The game draws at its own #SCREEN_SIZE; the renderer fits that
        // into the display (as on the Vita).
        Self::fit_game_screen(&mut host, width, height);
        // The loaded global data is the baseline: changes from the first
        // frames on (the game marks its first boot right away) are written.
        let global_fingerprint = host.persist_global_if_changed(None);
        let dump_frames: Vec<u64> = std::fs::read_to_string("sdmc:/switch/siglus_rs/dump-frames")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect();
        if !dump_frames.is_empty() {
            let _ = std::fs::create_dir_all(DUMP_DIR);
        }
        Ok(Self {
            host,
            frame: 0,
            dump_frames,
            global_fingerprint,
            held_buttons: 0,
            consumed_compatibility_keys: 0,
            pointer: None,
            pointer_left_down: false,
            touch_active: false,
        })
    }

    /// Drive one original-engine frame.  The libnx frontend supplies the
    /// elapsed time and maps controller/touch input to the shared VM input API.
    pub fn step(&mut self, dt_ms: u32) -> Result<bool> {
        if self.dump_frames.contains(&self.frame) {
            let path = format!("{DUMP_DIR}/frame-{}.png", self.frame);
            self.host.renderer_mut().dump_next_frame(PathBuf::from(path));
        }
        self.frame += 1;
        let running = self.host.step(dt_ms);
        if self.frame % GLOBAL_SAVE_INTERVAL == 0 {
            self.global_fingerprint = self.host.persist_global_if_changed(Some(self.global_fingerprint));
        }
        running
    }

    /// The display size changed; the game keeps its own screen size.
    pub fn resize(&mut self, width: u32, height: u32) {
        Self::fit_game_screen(&mut self.host, width, height);
    }

    fn fit_game_screen(host: &mut SiglusHost, width: u32, height: u32) {
        let (logical_w, logical_h) = host.logical_size();
        host.resize_with_logical_viewport(width, height, 1.0, logical_w, logical_h, 0, 0, width, height);
    }

    pub fn key_down(&mut self, key: VmKey) {
        self.host.key_down(key);
    }

    pub fn key_up(&mut self, key: VmKey) {
        self.host.key_up(key);
    }

    /// A touch at display pixel (x, y), mapped into the letterboxed game
    /// screen.
    pub fn touch(&mut self, phase: i32, x: f64, y: f64) {
        self.touch_active = matches!(phase, 0 | 1);
        if self.pointer_left_down {
            self.host.mouse_up(VmMouseButton::Left);
            self.pointer_left_down = false;
        }
        self.leave_pointer_mode();
        let [sx, sy, sw, sh] = self.host.renderer_mut().screen_viewport();
        let (logical_w, logical_h) = self.host.renderer_mut().logical_size();
        let lx = ((x - f64::from(sx)) * f64::from(logical_w) / f64::from(sw.max(1.0)))
            .clamp(0.0, f64::from(logical_w) - 1.0);
        let ly = ((y - f64::from(sy)) * f64::from(logical_h) / f64::from(sh.max(1.0)))
            .clamp(0.0, f64::from(logical_h) - 1.0);
        self.host.touch(phase, lx, ly);
    }

    fn leave_pointer_mode(&mut self) {
        self.pointer = None;
        self.host.controller_cursor(false);
    }

    pub fn move_pointer(&mut self, x: i32, y: i32, seconds: f64) {
        if self.touch_active || (x == 0 && y == 0) || seconds <= 0.0 {
            return;
        }

        let (width, height) = self.host.logical_size();
        let current = self.host.mouse_position();
        // Keep subpixel movement, but resynchronize if the game moved the mouse.
        let (px, py) = self
            .pointer
            .filter(|&(px, py)| current == Some((px.round() as i32, py.round() as i32)))
            .or_else(|| current.map(|(px, py)| (f64::from(px), f64::from(py))))
            .unwrap_or((f64::from(width) / 2.0, f64::from(height) / 2.0));
        // A fully tilted stick travels one logical screen height per second.
        let distance = f64::from(height) * seconds / 32768.0;
        let px = (px + f64::from(x) * distance).clamp(0.0, f64::from(width - 1));
        let py = (py - f64::from(y) * distance).clamp(0.0, f64::from(height - 1));
        self.pointer = Some((px, py));
        self.host.controller_cursor(true);
        if current != Some((px.round() as i32, py.round() as i32)) {
            self.host.mouse_move(px, py);
        }
    }

    pub fn gamepad_button(&mut self, button: u8, down: bool) {
        let previous = self.held_buttons;
        let bit = 1u32.checked_shl(u32::from(button)).unwrap_or(0);
        if down {
            self.held_buttons |= bit;
        } else {
            self.held_buttons &= !bit;
        }

        let key = match button {
            0 => Some(VmKey::Enter),
            1 | 10 => Some(VmKey::Escape),
            2 | 9 => Some(VmKey::Space),
            3 | 11 => Some(VmKey::Tab),
            6 => Some(VmKey::Shift),
            7 | 8 => Some(VmKey::Control),
            12 | 16 => Some(VmKey::ArrowLeft),
            13 | 17 => Some(VmKey::ArrowUp),
            14 | 18 => Some(VmKey::ArrowRight),
            15 | 19 => Some(VmKey::ArrowDown),
            // Right-stick direction bits still reach JOY_BUTTON below.
            _ => None,
        };
        if let Some(key) = key {
            let (logical_button, aliases) = match button {
                12..=19 => {
                    let direction = 12 + (button - 12) % 4;
                    (direction, (1 << direction) | (1 << (direction + 4)))
                }
                _ => (button, bit),
            };
            let logical_bit = 1 << logical_button;
            let was_held = previous & aliases != 0;
            let is_held = self.held_buttons & aliases != 0;
            if is_held && !was_held {
                if (12..=19).contains(&button) && !self.pointer_left_down {
                    self.leave_pointer_mode();
                }
                let consumed = if button == 0 && self.pointer.is_some() {
                    self.host.mouse_down(VmMouseButton::Left);
                    self.pointer_left_down = true;
                    true
                } else {
                    self.host.controller_key_down(key)
                };
                if consumed {
                    self.consumed_compatibility_keys |= logical_bit;
                }
            } else if !is_held && was_held {
                if button == 0 && self.pointer_left_down {
                    self.host.mouse_up(VmMouseButton::Left);
                    self.pointer_left_down = false;
                } else if self.consumed_compatibility_keys & logical_bit == 0 {
                    self.host.key_up(key);
                }
                self.consumed_compatibility_keys &= !logical_bit;
            }
        }

        // Keep the native controller edge available even when it also moves the UI.
        self.host.joypad_button(button as usize, down);
    }

    pub fn host_mut(&mut self) -> &mut SiglusHost {
        &mut self.host
    }
}

/// C ABI used by the libnx/deko3d application shell.  The shell owns Horizon
/// lifecycle and controller polling; this object owns the existing engine host
/// and therefore the original VM/resource/script flow.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_switch_engine_create(
    project_dir: *const c_char,
    width: u32,
    height: u32,
) -> *mut SwitchHost {
    if project_dir.is_null() {
        return std::ptr::null_mut();
    }
    let path = unsafe { CStr::from_ptr(project_dir) }
        .to_string_lossy()
        .into_owned();
    let config = SiglusHostConfig::new(PathBuf::from(path));
    match SwitchHost::new(config, width, height) {
        Ok(host) => Box::into_raw(Box::new(host)),
        Err(err) => {
            report_switch_error("Switch engine startup failed", &err);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_switch_engine_step(host: *mut SwitchHost, dt_ms: u32) -> bool {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return true;
    };
    match host.step(dt_ms) {
        Ok(exit) => exit,
        Err(err) => {
            report_switch_error("Switch engine frame failed", &err);
            true
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_switch_engine_gamepad(
    host: *mut SwitchHost,
    button: u8,
    down: bool,
) {
    if let Some(host) = unsafe { host.as_mut() } {
        host.gamepad_button(button, down);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_switch_engine_move_pointer(
    host: *mut SwitchHost,
    x: i32,
    y: i32,
    seconds: f64,
) {
    if let Some(host) = unsafe { host.as_mut() } {
        host.move_pointer(x, y, seconds);
    }
}

/// Touch phases match the host's existing platform-neutral convention:
/// 0=begin, 1=move, 2=end. libnx performs sampling; the shared host performs
/// the mouse/script translation so touch scripts behave the same as desktop.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_switch_engine_touch(
    host: *mut SwitchHost,
    phase: i32,
    x: f64,
    y: f64,
) {
    if let Some(host) = unsafe { host.as_mut() } {
        host.touch(phase, x, y);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglus_switch_engine_destroy(host: *mut SwitchHost) {
    if !host.is_null() {
        let mut host = unsafe { Box::from_raw(host) };
        let last = host.global_fingerprint;
        host.host.persist_global_if_changed(Some(last));
        drop(host);
    }
}
