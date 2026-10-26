//! Native AVG32 input policy, independent of any window host.
/// Mouse state.
#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Mouse {
    pub x: i32,
    pub y: i32,
    /// -1: none, 0: left click, 1: right click.
    pub(crate) click: i32,
    pub(crate) click_time: u64,
    /// Set while the engine waits for a click (`TextIconStart`); the
    /// context menu is only offered then.
    pub(crate) enable: bool,
    pub(crate) popup: bool,
    pub visible: bool,
}

impl Default for Mouse {
    fn default() -> Self {
        Self {
            x: -999,
            y: -999,
            click: -1,
            click_time: 0,
            enable: false,
            popup: false,
            visible: true,
        }
    }
}

impl Mouse {
    /// Deliver a host click using the original pending-click lifetime.
    pub fn press(&mut self, x: i32, y: i32, button: i32, now: u64) {
        self.x = x;
        self.y = y;
        self.click = button;
        self.click_time = now;
    }

    pub(crate) fn menu_flag(&self) -> bool {
        self.enable || self.popup
    }

    /// Position and pending click (clicks expire after 200 ms).
    pub fn state(&mut self, now: u64) -> (i32, i32, i32) {
        if now.saturating_sub(self.click_time) > 200 {
            self.click = -1;
        }
        (self.x, self.y, self.click)
    }

    /// Consumes a pending left click.
    pub fn button(&mut self, now: u64) -> bool {
        let clicked = self.click == 0 && now.saturating_sub(self.click_time) < 200;
        self.click = -1;
        clicked
    }

    pub fn flush(&mut self) {
        self.click = -1;
    }

    pub fn text_icon_start(&mut self) {
        self.enable = true;
    }

    pub fn text_icon_end(&mut self) {
        self.enable = false;
    }

    pub fn start_pdt_draw(&mut self) {
        self.enable = false;
    }

    pub fn disable_popup(&mut self) {
        self.popup = false;
    }

    pub fn popup_flag(&self) -> i32 {
        i32::from(!self.popup)
    }
}
