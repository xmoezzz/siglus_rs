//! Runtime input state (winit-agnostic).
//!
//! Siglus scripts query input via numeric forms (INPUT/MOUSE/KEYLIST) and helper
//! key objects. The original engine stores per-key state in fixed tables.
//!
//! For bring-up we keep:
//! - A fixed 0..=255 virtual-key table (down + edge "stock" flags)
//! - Mouse position
//! - Mouse wheel delta since last read / frame

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VmKey {
    Escape,
    Enter,
    Space,
    Backspace,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Function keys (F1..F12).
    F(u8),
    /// Digit keys 0..9.
    Digit(u8),
    /// Latin letter keys A..Z.
    Letter(char),
    /// Any other unmapped physical key.
    Other(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VmMouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

#[derive(Debug, Clone, Copy)]
struct KeyState {
    down: bool,
    down_stock: bool,
    up_stock: bool,
    down_up_stock: bool,
}

impl KeyState {
    const fn new() -> Self {
        Self {
            down: false,
            down_stock: false,
            up_stock: false,
            down_up_stock: false,
        }
    }

    fn clear_all(&mut self) {
        self.down = false;
        self.down_stock = false;
        self.up_stock = false;
        self.down_up_stock = false;
    }

    fn clear_stocks(&mut self) {
        self.down_stock = false;
        self.up_stock = false;
        self.down_up_stock = false;
    }
}

#[derive(Debug, Clone)]
pub struct InputState {
    keys: [KeyState; 256],

    pub mouse_x: i32,
    pub mouse_y: i32,

    wheel_delta: i32,

    /// Last key-down event since start.
    pub last_key_down: Option<VmKey>,
    /// Last mouse-down event since start.
    pub last_mouse_down: Option<VmMouseButton>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            keys: [KeyState::new(); 256],
            mouse_x: 0,
            mouse_y: 0,
            wheel_delta: 0,
            last_key_down: None,
            last_mouse_down: None,
        }
    }
}

impl InputState {
    // ---------------------------------------------------------------------
    // Virtual key helpers
    // ---------------------------------------------------------------------

    /// Returns true if the given virtual-key is currently held down.
    pub fn vk_is_down(&self, vk: u8) -> bool {
        self.keys[vk as usize].down
    }

    /// Returns true if the key transitioned to down since the last `next_frame`.
    pub fn vk_down_stock(&self, vk: u8) -> bool {
        self.keys[vk as usize].down_stock
    }

    /// Returns true if the key transitioned to up since the last `next_frame`.
    pub fn vk_up_stock(&self, vk: u8) -> bool {
        self.keys[vk as usize].up_stock
    }

    /// Returns true if a down+up pair happened since the last `next_frame`.
    pub fn vk_down_up_stock(&self, vk: u8) -> bool {
        self.keys[vk as usize].down_up_stock
    }

    fn vk_set_down(&mut self, vk: u8) {
        let st = &mut self.keys[vk as usize];
        if !st.down {
            st.down = true;
            st.down_stock = true;
        }
        // If both edges happen within the same frame, mark down_up_stock.
        if st.down_stock && st.up_stock {
            st.down_up_stock = true;
        }
    }

    fn vk_set_up(&mut self, vk: u8) {
        let st = &mut self.keys[vk as usize];
        if st.down {
            st.down = false;
            st.up_stock = true;
            if st.down_stock {
                st.down_up_stock = true;
            }
        }
    }

    /// Clears all keys (including held-down state) and all edge stocks.
    pub fn clear_all(&mut self) {
        for st in &mut self.keys {
            st.clear_all();
        }
        self.wheel_delta = 0;
        self.last_key_down = None;
        self.last_mouse_down = None;
    }

    /// Advances to the next frame: clears edge stocks but keeps held-down state.
    pub fn next_frame(&mut self) {
        for st in &mut self.keys {
            st.clear_stocks();
        }
        self.wheel_delta = 0;
    }

    // ---------------------------------------------------------------------
    // Wheel
    // ---------------------------------------------------------------------

    pub fn on_mouse_wheel(&mut self, delta_y: i32) {
        self.wheel_delta = self.wheel_delta.saturating_add(delta_y);
    }

    /// Reads and clears the accumulated wheel delta.
    pub fn take_wheel_delta(&mut self) -> i32 {
        let v = self.wheel_delta;
        self.wheel_delta = 0;
        v
    }

    // ---------------------------------------------------------------------
    // Bridge from platform key/mouse events
    // ---------------------------------------------------------------------

    pub fn is_key_down(&self, k: VmKey) -> bool {
        vmkey_to_vk(k).map(|vk| self.vk_is_down(vk)).unwrap_or(false)
    }

    pub fn is_mouse_down(&self, b: VmMouseButton) -> bool {
        match b {
            VmMouseButton::Left => self.vk_is_down(0x01),
            VmMouseButton::Right => self.vk_is_down(0x02),
            VmMouseButton::Middle => self.vk_is_down(0x04),
            VmMouseButton::Other(_) => false,
        }
    }

    pub fn on_key_down(&mut self, k: VmKey) {
        if let Some(vk) = vmkey_to_vk(k) {
            self.vk_set_down(vk);
        }
        self.last_key_down = Some(k);
    }

    pub fn on_key_up(&mut self, k: VmKey) {
        if let Some(vk) = vmkey_to_vk(k) {
            self.vk_set_up(vk);
        }
    }

    pub fn on_mouse_down(&mut self, b: VmMouseButton) {
        match b {
            VmMouseButton::Left => self.vk_set_down(0x01),
            VmMouseButton::Right => self.vk_set_down(0x02),
            VmMouseButton::Middle => self.vk_set_down(0x04),
            VmMouseButton::Other(_) => {}
        }
        self.last_mouse_down = Some(b);
    }

    pub fn on_mouse_up(&mut self, b: VmMouseButton) {
        match b {
            VmMouseButton::Left => self.vk_set_up(0x01),
            VmMouseButton::Right => self.vk_set_up(0x02),
            VmMouseButton::Middle => self.vk_set_up(0x04),
            VmMouseButton::Other(_) => {}
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    /// Returns a direction bitmask based on arrow keys.
    ///
    /// Bit layout (best-effort bring-up):
    ///   1=left, 2=right, 4=up, 8=down
    pub fn dir_mask(&self) -> i64 {
        let mut m = 0;
        if self.vk_is_down(0x25) {
            m |= 1;
        }
        if self.vk_is_down(0x27) {
            m |= 2;
        }
        if self.vk_is_down(0x26) {
            m |= 4;
        }
        if self.vk_is_down(0x28) {
            m |= 8;
        }
        m
    }
}

fn vmkey_to_vk(k: VmKey) -> Option<u8> {
    match k {
        VmKey::Escape => Some(0x1B),
        VmKey::Enter => Some(0x0D),
        VmKey::Space => Some(0x20),
        VmKey::Backspace => Some(0x08),
        VmKey::Tab => Some(0x09),

        VmKey::ArrowLeft => Some(0x25),
        VmKey::ArrowUp => Some(0x26),
        VmKey::ArrowRight => Some(0x27),
        VmKey::ArrowDown => Some(0x28),

        VmKey::F(n) if (1..=12).contains(&n) => Some(0x6F + n), // F1=0x70
        VmKey::Digit(n) if n <= 9 => Some(0x30 + n),
        VmKey::Letter(c) => {
            let uc = c.to_ascii_uppercase();
            if ('A'..='Z').contains(&uc) {
                Some(uc as u8)
            } else {
                None
            }
        }
        VmKey::Other(_) => None,
        _ => None,
    }
}
