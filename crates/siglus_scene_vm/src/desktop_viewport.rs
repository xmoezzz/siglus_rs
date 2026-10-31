#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_game_canvas_to_half_size() {
        let v = DesktopViewport::fit(960, 540, 1920, 1080);
        assert_eq!(v.rect(), (0, 0, 960, 540));
        assert_eq!(v.game_position(480.0, 270.0), (960.0, 540.0));
        assert_eq!(v.game_position(960.0, 540.0), (1920.0, 1080.0));
    }

    #[test]
    fn centers_wide_and_tall_windows_without_distortion() {
        let wide = DesktopViewport::fit(1600, 600, 1920, 1080);
        assert_eq!(wide.rect(), (266, 0, 1067, 600));
        assert_eq!(wide.game_position(266.0, 0.0), (0.0, 0.0));
        let tall = DesktopViewport::fit(960, 800, 1920, 1080);
        assert_eq!(tall.rect(), (0, 130, 960, 540));
        assert_eq!(tall.game_position(480.0, 400.0), (960.0, 540.0));
    }

    #[test]
    fn margins_do_not_map_to_game_buttons() {
        let v = DesktopViewport::fit(960, 800, 1920, 1080);
        assert!(v.game_position(0.0, 0.0).1 < 0.0);
        assert!(v.game_position(960.0, 800.0).1 > 1080.0);
    }

    #[test]
    fn minimized_surface_remains_finite() {
        let v = DesktopViewport::fit(0, 0, 1920, 1080);
        assert_eq!(v.rect(), (0, 0, 1, 1));
        let p = v.game_position(0.0, 0.0);
        assert!(p.0.is_finite() && p.1.is_finite());
    }

    #[test]
    fn maps_game_ime_position_back_to_window() {
        let v = DesktopViewport::fit(960, 800, 1920, 1080);
        assert_eq!(v.surface_position(960.0, 540.0), (480.0, 400.0));
        assert_eq!(v.surface_extent(200.0, 40.0), (100.0, 20.0));
    }
}
/// Fits a fixed game canvas into a resizable physical-pixel surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DesktopViewport {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    game_width: u32,
    game_height: u32,
}

impl DesktopViewport {
    pub(crate) fn fit(sw: u32, sh: u32, gw: u32, gh: u32) -> Self {
        let (sw, sh, gw, gh) = (sw.max(1), sh.max(1), gw.max(1), gh.max(1));
        let scale = (sw as f64 / gw as f64).min(sh as f64 / gh as f64);
        let width = (gw as f64 * scale).round().max(1.0).min(sw as f64) as u32;
        let height = (gh as f64 * scale).round().max(1.0).min(sh as f64) as u32;
        Self {
            x: (sw - width) / 2,
            y: (sh - height) / 2,
            width,
            height,
            game_width: gw,
            game_height: gh,
        }
    }

    pub(crate) fn rect(self) -> (u32, u32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }

    pub(crate) fn game_position(self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.x as f64) * self.game_width as f64 / self.width as f64,
            (y - self.y as f64) * self.game_height as f64 / self.height as f64,
        )
    }

    pub(crate) fn surface_position(self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = self.surface_extent(x, y);
        (self.x as f64 + x, self.y as f64 + y)
    }

    pub(crate) fn surface_extent(self, w: f64, h: f64) -> (f64, f64) {
        (
            w * self.width as f64 / self.game_width as f64,
            h * self.height as f64 / self.game_height as f64,
        )
    }
}
