//! Minimal UI runtime.
//!
//! This stage focuses on:
//! - a message layer (text window background)
//! - simple state for message display / waits
//!
//! It is intentionally conservative and does not attempt to fully re-implement
//! Siglus text rendering yet.

use crate::image_manager::ImageId;
use crate::layer::{LayerId, SpriteFit, SpriteId, SpriteSizeMode};
use crate::runtime::globals::{ScriptRuntimeState, SyscomRuntimeState};
use crate::text_render::FontCache;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A minimal UI runtime that owns a couple of fixed layers/sprites.
#[derive(Debug, Default)]
pub struct UiRuntime {
    /// Dedicated layer for message/text UI.
    pub ui_layer: Option<LayerId>,
    /// Background sprite of the message window.
    pub msg_bg_sprite: Option<SpriteId>,
    /// Filter/mask sprite of the message window.
    pub msg_filter_sprite: Option<SpriteId>,
    /// Text sprite for the message body.
    pub msg_text_sprite: Option<SpriteId>,
    /// Text sprite for the speaker name.
    pub name_text_sprite: Option<SpriteId>,
    /// Whether message window background is visible.
    pub msg_bg_visible: bool,
    /// Cached bg image id for message window background.
    pub msg_bg_image: Option<ImageId>,
    /// Cached filter image id for message window.
    pub msg_filter_image: Option<ImageId>,
    /// Cached image id for the rendered message text.
    pub msg_text_image: Option<ImageId>,
    /// Cached image id for the rendered name text.
    pub name_text_image: Option<ImageId>,
    /// Current message string (debug-only for now).
    pub current_message: Option<String>,
    /// Current speaker/name string (debug-only for now).
    pub current_name: Option<String>,
    /// Whether we are in a blocking message wait.
    pub waiting_message: bool,
    /// Timestamp of the current message wait.
    pub wait_started_at: Option<Instant>,
    /// Cached message length for auto-advance timing.
    pub wait_message_len: usize,
    /// Timestamp when the current message reveal started.
    msg_reveal_start: Option<Instant>,
    /// Visible character count for typewriter-like reveal.
    msg_visible_chars: usize,
    /// Visible count at the start of the current reveal segment.
    msg_reveal_base: usize,

    /// If set, we clear the current message when a message-wait ends.
    ///
    /// This approximates page-break behavior (e.g., PP/R/PAGE in MWND).
    pub clear_message_on_wait_end: bool,

    /// Message text needs re-render.
    msg_text_dirty: bool,
    /// Name text needs re-render.
    name_text_dirty: bool,

    /// System overlay (config/save/load) active flag.
    sys_overlay_active: bool,
    /// Overlay background sprite.
    sys_bg_sprite: Option<SpriteId>,
    /// Overlay text sprite.
    sys_text_sprite: Option<SpriteId>,
    /// Overlay background image id.
    sys_bg_image: Option<ImageId>,
    /// Overlay text image id.
    sys_text_image: Option<ImageId>,
    /// Overlay text content.
    sys_text: String,
    /// Overlay text needs re-render.
    sys_text_dirty: bool,

    /// Cached font file paths from project_dir/font.
    font_paths: Vec<PathBuf>,
    font_scanned: bool,

    font_cache: FontCache,
}

impl UiRuntime {
    fn ensure_layer(
        layers: &mut crate::layer::LayerManager,
        want: &mut Option<LayerId>,
    ) -> LayerId {
        if let Some(id) = *want {
            if layers.layer(id).is_some() {
                return id;
            }
        }
        let id = layers.create_layer();
        *want = Some(id);
        id
    }

    fn ensure_msg_bg_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.msg_bg_sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.msg_bg_sprite = Some(sprite_id);
        sprite_id
    }

    fn ensure_msg_filter_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.msg_filter_sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.msg_filter_sprite = Some(sprite_id);
        sprite_id
    }

    fn ensure_text_sprite(
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
        slot: &mut Option<SpriteId>,
    ) -> SpriteId {
        if let Some(id) = *slot {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        *slot = Some(sprite_id);
        sprite_id
    }

    fn msg_rect(w: u32, h: u32) -> (i32, i32, u32, u32) {
        let bg_h = (h / 3).max(1);
        let bg_y = h as i32 - bg_h as i32;
        let pad = 24i32;
        let name_h = 36i32;
        let x = pad;
        let y = bg_y + pad + name_h;
        let width = (w as i32 - pad * 2).max(1) as u32;
        let height = (bg_h as i32 - pad * 2 - name_h).max(1) as u32;
        (x, y, width, height)
    }

    fn name_rect(w: u32, h: u32) -> (i32, i32, u32, u32) {
        let bg_h = (h / 3).max(1);
        let bg_y = h as i32 - bg_h as i32;
        let pad = 24i32;
        let name_h = 36i32;
        let x = pad;
        let y = bg_y + pad;
        let width = (w as i32 - pad * 2).max(1) as u32;
        let height = name_h as u32;
        (x, y, width, height)
    }

    /// Ensure fixed UI sprites exist and are laid out for the given screen size.
    pub fn sync_layout(&mut self, layers: &mut crate::layer::LayerManager, w: u32, h: u32) {
        let ui_layer = Self::ensure_layer(layers, &mut self.ui_layer);
        let bg_sprite = self.ensure_msg_bg_sprite(layers, ui_layer);
        let filter_sprite = self.ensure_msg_filter_sprite(layers, ui_layer);
        let msg_text_sprite = Self::ensure_text_sprite(layers, ui_layer, &mut self.msg_text_sprite);
        let name_text_sprite =
            Self::ensure_text_sprite(layers, ui_layer, &mut self.name_text_sprite);

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(bg_sprite))
        {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: w,
                height: (h / 3).max(1),
            };
            s.x = 0;
            s.y = (h as i32) - (h as i32 / 3);
            s.order = 1_000_000; // always above typical sprites
        }

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(filter_sprite))
        {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: w,
                height: (h / 3).max(1),
            };
            s.x = 0;
            s.y = (h as i32) - (h as i32 / 3);
            s.order = 1_000_005;
        }

        let (mx, my, mw, mh) = Self::msg_rect(w, h);
        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(msg_text_sprite))
        {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: mw,
                height: mh,
            };
            s.x = mx;
            s.y = my;
            s.order = 1_000_010;
        }

        let (nx, ny, nw, nh) = Self::name_rect(w, h);
        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(name_text_sprite))
        {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: nw,
                height: nh,
            };
            s.x = nx;
            s.y = ny;
            s.order = 1_000_020;
        }
    }

    /// Called once per frame to update UI and apply visibility.
    pub fn tick(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
        w: u32,
        h: u32,
        script: &ScriptRuntimeState,
        syscom: &SyscomRuntimeState,
    ) {
        self.sync_layout(layers, w, h);
        self.scan_font_dir(project_dir);
        if !self.font_cache.is_loaded() {
            let _ = self
                .font_cache
                .load_from_font_dir(&project_dir.join("font"));
        }
        self.update_message_reveal(script, syscom);
        self.refresh_text_images(images, w, h);
        self.sync_sys_overlay(layers, images, w, h);

        let Some(ui_layer) = self.ui_layer else {
            return;
        };
        let Some(bg_sprite) = self.msg_bg_sprite else {
            return;
        };

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(bg_sprite))
        {
            s.visible = self.msg_bg_visible;
            if let Some(img) = self.msg_bg_image {
                s.image_id = Some(img);
            }
        }

        if let Some(sprite_id) = self.msg_filter_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                let visible = self.msg_bg_visible && self.msg_filter_image.is_some();
                s.visible = visible;
                s.image_id = self.msg_filter_image;

                const GET_FILTER_COLOR_R: i32 = 272;
                const GET_FILTER_COLOR_G: i32 = 273;
                const GET_FILTER_COLOR_B: i32 = 274;
                const GET_FILTER_COLOR_A: i32 = 275;
                let cfg = &syscom.config_int;
                let r = cfg
                    .get(&GET_FILTER_COLOR_R)
                    .copied()
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;
                let g = cfg
                    .get(&GET_FILTER_COLOR_G)
                    .copied()
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;
                let b = cfg
                    .get(&GET_FILTER_COLOR_B)
                    .copied()
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;
                let a = cfg
                    .get(&GET_FILTER_COLOR_A)
                    .copied()
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;

                s.alpha = a;
                s.tr = 255;
                s.color_rate = 255;
                s.color_r = r;
                s.color_g = g;
                s.color_b = b;
                s.mask_mode = 1;
            }
        }

        if let Some(sprite_id) = self.msg_text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = self.msg_bg_visible && self.msg_text_image.is_some();
                s.image_id = self.msg_text_image;
            }
        }

        if let Some(sprite_id) = self.name_text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = self.msg_bg_visible && self.name_text_image.is_some();
                s.image_id = self.name_text_image;
            }
        }

        if let Some(sys_bg) = self.sys_bg_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sys_bg))
            {
                s.visible = self.sys_overlay_active;
                if let Some(img) = self.sys_bg_image {
                    s.image_id = Some(img);
                }
            }
        }
        if let Some(sys_text) = self.sys_text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sys_text))
            {
                s.visible = self.sys_overlay_active && self.sys_text_image.is_some();
                s.image_id = self.sys_text_image;
            }
        }
    }

    pub fn set_message_bg(&mut self, img: ImageId) {
        self.msg_bg_image = Some(img);
    }

    pub fn show_message_bg(&mut self, on: bool) {
        self.msg_bg_visible = on;
    }

    pub fn set_message_filter(&mut self, img: Option<ImageId>) {
        self.msg_filter_image = img;
    }

    pub fn set_message(&mut self, msg: String) {
        self.current_message = Some(msg);
        self.msg_text_dirty = true;
        self.msg_visible_chars = 0;
        self.msg_reveal_base = 0;
        self.msg_reveal_start = Some(Instant::now());
    }

    pub fn append_message(&mut self, msg: &str) {
        if msg.is_empty() {
            return;
        }
        match self.current_message.as_mut() {
            Some(s) => s.push_str(msg),
            None => self.current_message = Some(msg.to_string()),
        }
        self.msg_text_dirty = true;
        self.msg_reveal_base = self.msg_visible_chars;
        self.msg_reveal_start = Some(Instant::now());
    }

    pub fn append_linebreak(&mut self) {
        match self.current_message.as_mut() {
            Some(s) => s.push('\n'),
            None => self.current_message = Some("\n".to_string()),
        }
        self.msg_text_dirty = true;
        self.msg_reveal_base = self.msg_visible_chars;
        self.msg_reveal_start = Some(Instant::now());
    }

    pub fn set_name(&mut self, name: String) {
        self.current_name = if name.is_empty() { None } else { Some(name) };
        self.name_text_dirty = true;
    }

    pub fn clear_name(&mut self) {
        self.current_name = None;
        self.name_text_dirty = true;
    }

    pub fn clear_message(&mut self) {
        self.current_message = None;
        self.msg_text_dirty = true;
        self.msg_visible_chars = 0;
        self.msg_reveal_base = 0;
        self.msg_reveal_start = None;
    }

    pub fn begin_wait_message(&mut self) {
        self.waiting_message = true;
        self.wait_started_at = Some(Instant::now());
        self.wait_message_len = self
            .current_message
            .as_deref()
            .unwrap_or("")
            .chars()
            .count();
    }

    pub fn end_wait_message(&mut self) {
        self.waiting_message = false;
        self.wait_started_at = None;

        if self.clear_message_on_wait_end {
            self.clear_message_on_wait_end = false;
            self.clear_message();
        }
    }

    pub fn request_clear_message_on_wait_end(&mut self) {
        self.clear_message_on_wait_end = true;
    }

    pub fn set_sys_overlay(&mut self, active: bool, text: String) {
        self.sys_overlay_active = active;
        if self.sys_text != text {
            self.sys_text = text;
            self.sys_text_dirty = true;
        }
    }

    pub fn auto_advance_due(
        &self,
        script: &ScriptRuntimeState,
        syscom: &SyscomRuntimeState,
    ) -> bool {
        if !self.waiting_message {
            return false;
        }
        if script.msg_nowait {
            return true;
        }
        let auto_mode = script.auto_mode_flag || syscom.auto_mode.onoff;
        if !auto_mode {
            return false;
        }
        let Some(start) = self.wait_started_at else {
            return false;
        };
        let (moji_wait, min_wait) = auto_mode_timing(script, syscom);
        let len = self.wait_message_len.max(1) as i64;
        let by_len = moji_wait.saturating_mul(len);
        let total = by_len.max(min_wait).max(0) as u64;
        start.elapsed() >= Duration::from_millis(total)
    }

    fn update_message_reveal(&mut self, script: &ScriptRuntimeState, syscom: &SyscomRuntimeState) {
        let total = self
            .current_message
            .as_deref()
            .unwrap_or("")
            .chars()
            .count();
        if total == 0 {
            self.msg_visible_chars = 0;
            self.msg_reveal_base = 0;
            self.msg_reveal_start = None;
            return;
        }

        if script.msg_nowait {
            if self.msg_visible_chars != total {
                self.msg_visible_chars = total;
                self.msg_text_dirty = true;
            }
            self.msg_reveal_base = total;
            self.msg_reveal_start = None;
            return;
        }

        let Some(ms_per_char) = message_speed_ms(script, syscom) else {
            if self.msg_visible_chars != total {
                self.msg_visible_chars = total;
                self.msg_text_dirty = true;
            }
            self.msg_reveal_base = total;
            self.msg_reveal_start = None;
            return;
        };

        let Some(start) = self.msg_reveal_start else {
            return;
        };
        let elapsed = start.elapsed().as_millis() as usize;
        let inc = if ms_per_char == 0 {
            total
        } else {
            elapsed / ms_per_char as usize
        };
        let visible = self.msg_reveal_base.saturating_add(inc).min(total);
        if self.msg_visible_chars != visible {
            self.msg_visible_chars = visible;
            self.msg_text_dirty = true;
        }
        if visible >= total {
            self.msg_reveal_base = total;
            self.msg_reveal_start = None;
        }
    }

    fn current_message_visible(&self) -> String {
        let Some(msg) = self.current_message.as_deref() else {
            return String::new();
        };
        if self.msg_visible_chars == 0 {
            return String::new();
        }
        msg.chars().take(self.msg_visible_chars).collect()
    }

    fn refresh_text_images(
        &mut self,
        images: &mut crate::image_manager::ImageManager,
        w: u32,
        h: u32,
    ) {
        if self.msg_text_dirty {
            let (x, y, mw, mh) = Self::msg_rect(w, h);
            let _ = (x, y);
            self.msg_text_image =
                self.font_cache
                    .render_text(images, &self.current_message_visible(), 26.0, mw, mh);
            self.msg_text_dirty = false;
        }

        if self.name_text_dirty {
            let (x, y, mw, mh) = Self::name_rect(w, h);
            let _ = (x, y);
            self.name_text_image = self.font_cache.render_text(
                images,
                self.current_name.as_deref().unwrap_or(""),
                24.0,
                mw,
                mh,
            );
            self.name_text_dirty = false;
        }
    }

    fn sync_sys_overlay(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        images: &mut crate::image_manager::ImageManager,
        w: u32,
        h: u32,
    ) {
        if !self.sys_overlay_active {
            return;
        }
        let ui_layer = Self::ensure_layer(layers, &mut self.ui_layer);
        let bg = self.ensure_sys_bg_sprite(layers, ui_layer);
        let text = Self::ensure_text_sprite(layers, ui_layer, &mut self.sys_text_sprite);

        if self.sys_bg_image.is_none() {
            self.sys_bg_image = Some(images.solid_rgba((0, 0, 0, 180)));
        }

        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(bg)) {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: w,
                height: h,
            };
            s.x = 0;
            s.y = 0;
            s.order = 2_000_000;
        }

        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(text)) {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: w.saturating_sub(80),
                height: h.saturating_sub(80),
            };
            s.x = 40;
            s.y = 40;
            s.order = 2_000_010;
        }

        if self.sys_text_dirty {
            self.sys_text_image = self.font_cache.render_text(
                images,
                &self.sys_text,
                24.0,
                w.saturating_sub(80),
                h.saturating_sub(80),
            );
            self.sys_text_dirty = false;
        }
    }

    fn ensure_sys_bg_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.sys_bg_sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.sys_bg_sprite = Some(sprite_id);
        sprite_id
    }
}

fn auto_mode_timing(script: &ScriptRuntimeState, syscom: &SyscomRuntimeState) -> (i64, i64) {
    const GET_AUTO_MODE_MOJI_WAIT: i32 = 254;
    const GET_AUTO_MODE_MIN_WAIT: i32 = 257;

    let moji_wait = if script.auto_mode_moji_wait >= 0 {
        script.auto_mode_moji_wait
    } else {
        *syscom
            .config_int
            .get(&GET_AUTO_MODE_MOJI_WAIT)
            .unwrap_or(&-1)
    };
    let min_wait = if script.auto_mode_min_wait >= 0 {
        script.auto_mode_min_wait
    } else {
        *syscom.config_int.get(&GET_AUTO_MODE_MIN_WAIT).unwrap_or(&0)
    };

    let moji_wait = if moji_wait >= 0 { moji_wait } else { 80 };
    let min_wait = if min_wait >= 0 { min_wait } else { 0 };
    (moji_wait, min_wait)
}

fn message_speed_ms(script: &ScriptRuntimeState, syscom: &SyscomRuntimeState) -> Option<u64> {
    const GET_MESSAGE_SPEED: i32 = 248;

    let speed = if script.msg_speed >= 0 {
        script.msg_speed
    } else {
        *syscom.config_int.get(&GET_MESSAGE_SPEED).unwrap_or(&0)
    };
    if speed <= 0 {
        return None;
    }
    let s = speed.clamp(0, 100) as u64;
    let base = 80u64;
    let span = 70u64;
    let ms = base.saturating_sub(span.saturating_mul(s) / 100).max(5);
    Some(ms)
}

impl UiRuntime {
    fn scan_font_dir(&mut self, project_dir: &Path) {
        if self.font_scanned {
            return;
        }
        self.font_scanned = true;
        let dir = project_dir.join("font");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext == "ttf" || ext == "otf" || ext == "ttc" {
                self.font_paths.push(path);
            }
        }
    }
}
