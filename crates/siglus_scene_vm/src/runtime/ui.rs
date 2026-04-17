//! Message-window rendering state projected from runtime MWND state.

use crate::image_manager::ImageId;
use crate::layer::{LayerId, SpriteFit, SpriteId, SpriteSizeMode};
use crate::runtime::globals::{ScriptRuntimeState, SyscomRuntimeState};
use crate::text_render::FontCache;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct UiRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl UiRect {
    fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
}

#[derive(Debug, Clone, Copy)]
struct UiWindowAnim {
    dx: i32,
    dy: i32,
    scale_x: f32,
    scale_y: f32,
    rotate: f32,
    alpha: u8,
    pivot_abs_x: f32,
    pivot_abs_y: f32,
}

/// UI-side sprite cache for the message-window family and related overlays.
#[derive(Debug, Default)]
pub struct MwndWakuRuntime {
    pub bg_sprite: Option<SpriteId>,
    pub filter_sprite: Option<SpriteId>,
    pub bg_image: Option<ImageId>,
    pub filter_image: Option<ImageId>,
    pub bg_file: Option<String>,
    pub filter_file: Option<String>,
    pub bg_size: Option<(u32, u32)>,
}

#[derive(Debug, Default)]
pub struct MwndFaceRuntime {
    pub sprite: Option<SpriteId>,
    pub image: Option<ImageId>,
    pub file: Option<String>,
    pub no: i64,
    pub rep_pos: Option<(i64, i64)>,
}

#[derive(Debug, Default)]
pub struct MwndNameRuntime {
    pub text_sprite: Option<SpriteId>,
    pub text_image: Option<ImageId>,
    pub text: Option<String>,
    pub text_dirty: bool,
}

#[derive(Debug, Default)]
pub struct MwndMsgRuntime {
    pub text_sprite: Option<SpriteId>,
    pub text_image: Option<ImageId>,
    pub text: Option<String>,
    pub waiting: bool,
    pub wait_started_at: Option<Instant>,
    pub wait_message_len: usize,
    pub reveal_start: Option<Instant>,
    pub visible_chars: usize,
    pub reveal_base: usize,
    pub slide_started_at: Option<Instant>,
    pub slide_enabled: bool,
    pub slide_time_ms: u64,
    pub clear_on_wait_end: bool,
    pub text_dirty: bool,
}

#[derive(Debug, Default)]
pub struct MwndWindowRuntime {
    pub pos: Option<(i32, i32)>,
    pub size: Option<(u32, u32)>,
    pub moji_cnt: Option<(i64, i64)>,
    pub moji_size: Option<i64>,
    pub moji_color: Option<i64>,
}

#[derive(Debug, Default)]
pub struct MwndAnimRuntime {
    pub visible: bool,
    pub target_visible: bool,
    pub progress: f32,
    pub from: f32,
    pub to: f32,
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    pub anim_type: i64,
    pub clear_text_on_close_end: bool,
}

#[derive(Debug, Default)]
pub struct MwndRuntime {
    pub layer: Option<LayerId>,
    pub projection_active: bool,
    pub waku: MwndWakuRuntime,
    pub face: MwndFaceRuntime,
    pub name: MwndNameRuntime,
    pub msg: MwndMsgRuntime,
    pub window: MwndWindowRuntime,
    pub anim: MwndAnimRuntime,
}

#[derive(Debug, Default)]
pub struct SysOverlayRuntime {
    pub active: bool,
    pub bg_sprite: Option<SpriteId>,
    pub text_sprite: Option<SpriteId>,
    pub bg_image: Option<ImageId>,
    pub text_image: Option<ImageId>,
    pub text: String,
    pub text_dirty: bool,
}

#[derive(Debug, Default, Clone)]
pub struct MwndProjectionState {
    pub bg_file: Option<String>,
    pub filter_file: Option<String>,
    pub face_file: Option<String>,
    pub face_no: i64,
    pub rep_pos: Option<(i64, i64)>,
    pub window_pos: Option<(i64, i64)>,
    pub window_size: Option<(i64, i64)>,
    pub window_moji_cnt: Option<(i64, i64)>,
    pub moji_size: Option<i64>,
    pub moji_color: Option<i64>,
    pub slide_enabled: bool,
    pub slide_time: i64,
    pub name_text: String,
    pub msg_text: String,
}

#[derive(Debug, Default)]
pub struct UiRuntime {
    pub mwnd: MwndRuntime,
    pub sys: SysOverlayRuntime,
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
        if let Some(id) = self.mwnd.waku.bg_sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.mwnd.waku.bg_sprite = Some(sprite_id);
        sprite_id
    }

    fn ensure_msg_filter_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.mwnd.waku.filter_sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.mwnd.waku.filter_sprite = Some(sprite_id);
        sprite_id
    }

    fn ensure_msg_face_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.mwnd.face.sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.mwnd.face.sprite = Some(sprite_id);
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

    fn default_window_origin(
        screen_w: u32,
        screen_h: u32,
        window_w: u32,
        window_h: u32,
    ) -> (i32, i32) {
        let x = ((screen_w as i32 - window_w as i32) / 2).max(0);
        let y = (screen_h as i32 - window_h as i32).max(0);
        (x, y)
    }

    fn message_font_px(&self) -> u32 {
        self.mwnd.window.moji_size.unwrap_or(26).clamp(10, 96) as u32
    }

    fn name_font_px(&self) -> u32 {
        ((self.message_font_px() as f32) * 0.9)
            .round()
            .clamp(10.0, 72.0) as u32
    }

    fn base_padding(&self) -> i32 {
        ((self.message_font_px() as f32) * 0.75)
            .round()
            .clamp(12.0, 32.0) as i32
    }

    fn name_band_height(&self) -> i32 {
        if self.mwnd.name.text.as_deref().unwrap_or("").is_empty() {
            0
        } else {
            (self.name_font_px() as i32 + self.base_padding() / 2).max(20)
        }
    }

    fn estimated_text_extent(&self, text: &str, font_px: u32) -> (u32, u32) {
        let mut max_cols = 0u32;
        let mut lines = 0u32;
        for line in text.split('\n') {
            lines += 1;
            max_cols = max_cols.max(line.chars().count() as u32);
        }
        if lines == 0 {
            lines = 1;
        }
        let char_w = ((font_px as f32) * 0.58).round().max(1.0) as u32;
        let line_h = ((font_px as f32) * 1.35).round().max(1.0) as u32;
        (
            max_cols.max(1).saturating_mul(char_w),
            lines.saturating_mul(line_h),
        )
    }

    fn derive_window_size(&self, fallback_w: u32, fallback_h: u32) -> (u32, u32) {
        if let Some((ww, hh)) = self.mwnd.window.size {
            return (ww.max(1), hh.max(1));
        }
        if let Some((ww, hh)) = self.mwnd.waku.bg_size {
            return (ww.max(1), hh.max(1));
        }

        let font_px = self.message_font_px();
        let pad = self.base_padding().max(1) as u32;
        let name_h = self.name_band_height().max(0) as u32;
        let msg_text = self.mwnd.msg.text.as_deref().unwrap_or("");
        let (text_w, text_h) = self.estimated_text_extent(msg_text, font_px);

        let mut width = text_w.saturating_add(pad.saturating_mul(2));
        let mut height = text_h
            .saturating_add(name_h)
            .saturating_add(pad.saturating_mul(2));

        if let Some((cols, rows)) = self.mwnd.window.moji_cnt {
            let cols = cols.max(1) as u32;
            let rows = rows.max(1) as u32;
            let line_h = ((font_px as f32) * 1.35).round().max(1.0) as u32;
            width = width.max(
                cols.saturating_mul(font_px)
                    .saturating_add(pad.saturating_mul(2)),
            );
            height = height.max(
                rows.saturating_mul(line_h)
                    .saturating_add(name_h)
                    .saturating_add(pad.saturating_mul(2)),
            );
        }

        if self.mwnd.face.file.is_some() || self.mwnd.face.image.is_some() {
            width = width.saturating_add(self.face_reserved_width(UiRect::new(
                0,
                0,
                width.max(1),
                height.max(1),
            )) as u32);
        }

        (
            width.clamp(1, fallback_w.max(1)),
            height.clamp(1, fallback_h.max(1)),
        )
    }

    fn window_rect(&self, w: u32, h: u32) -> UiRect {
        let (ww, hh) = self.derive_window_size(w, h);
        let (mut x, mut y) = Self::default_window_origin(w, h, ww, hh);
        if let Some((px, py)) = self.mwnd.window.pos {
            x = px;
            y = py;
        }
        UiRect::new(x, y, ww, hh)
    }

    fn face_reserved_width(&self, rect: UiRect) -> i32 {
        if self.mwnd.face.image.is_none() && self.mwnd.face.file.is_none() {
            return 0;
        }
        let reserve = ((rect.h as f32) * 0.42).round() as i32;
        reserve.clamp(72, 260)
    }

    fn msg_rect(&self, w: u32, h: u32) -> (i32, i32, u32, u32) {
        let rect = self.window_rect(w, h);
        let pad = self.base_padding();
        let name_h = self.name_band_height();
        let face_pad = if self.mwnd.face.file.is_some() || self.mwnd.face.image.is_some() {
            self.face_reserved_width(rect) + pad / 2
        } else {
            0
        };
        let x = rect.x + pad + face_pad;
        let y = rect.y + pad + name_h;
        let width = (rect.w as i32 - pad * 2 - face_pad).max(1) as u32;
        let height = (rect.h as i32 - pad * 2 - name_h).max(1) as u32;
        (x, y, width, height)
    }

    fn name_rect(&self, w: u32, h: u32) -> (i32, i32, u32, u32) {
        let rect = self.window_rect(w, h);
        let pad = self.base_padding();
        let name_h = self.name_band_height().max(1);
        let face_pad = if self.mwnd.face.file.is_some() || self.mwnd.face.image.is_some() {
            self.face_reserved_width(rect) + pad / 2
        } else {
            0
        };
        let x = rect.x + pad + face_pad;
        let y = rect.y + pad;
        let width = (rect.w as i32 - pad * 2 - face_pad).max(1) as u32;
        let height = name_h as u32;
        (x, y, width, height)
    }

    fn face_rect(&self, w: u32, h: u32) -> UiRect {
        let rect = self.window_rect(w, h);
        let pad = self.base_padding();
        let reserve_w = self.face_reserved_width(rect).max(1) as u32;
        let max_h = (rect.h as i32 - pad * 2).max(1) as u32;
        let fw = reserve_w;
        let fh = reserve_w.min(max_h);
        let (mut x, mut y) = (rect.x + pad, rect.y + rect.h as i32 - fh as i32 - pad);
        if let Some((rx, ry)) = self.mwnd.face.rep_pos {
            x = rect.x + rx as i32;
            y = rect.y + ry as i32;
        }
        UiRect::new(x, y, fw, fh)
    }

    fn begin_message_window_anim(
        &mut self,
        target_visible: bool,
        anime_type: i64,
        duration_ms: u64,
        clear_on_close: bool,
    ) {
        let current = self.mwnd.anim.progress;
        self.mwnd.anim.target_visible = target_visible;
        self.mwnd.anim.anim_type = anime_type;
        self.mwnd.anim.clear_text_on_close_end = clear_on_close;
        if duration_ms == 0 {
            self.mwnd.anim.progress = if target_visible { 1.0 } else { 0.0 };
            self.mwnd.anim.from = self.mwnd.anim.progress;
            self.mwnd.anim.to = self.mwnd.anim.progress;
            self.mwnd.anim.started_at = None;
            self.mwnd.anim.duration_ms = 0;
            self.mwnd.anim.visible = target_visible;
            if !target_visible && clear_on_close {
                self.mwnd.anim.clear_text_on_close_end = false;
                self.clear_message();
                self.clear_name();
            }
            return;
        }
        self.mwnd.anim.visible = true;
        self.mwnd.anim.from = current;
        self.mwnd.anim.to = if target_visible { 1.0 } else { 0.0 };
        self.mwnd.anim.started_at = Some(Instant::now());
        self.mwnd.anim.duration_ms = duration_ms;
    }

    fn update_message_window_anim(&mut self) {
        let Some(start) = self.mwnd.anim.started_at else {
            self.mwnd.anim.visible = self.mwnd.anim.progress > 0.0 && self.mwnd.anim.target_visible;
            return;
        };
        let dur = self.mwnd.anim.duration_ms.max(1);
        let t = (start.elapsed().as_secs_f32() / (dur as f32 / 1000.0)).clamp(0.0, 1.0);
        self.mwnd.anim.progress =
            self.mwnd.anim.from + (self.mwnd.anim.to - self.mwnd.anim.from) * t;
        self.mwnd.anim.visible = self.mwnd.anim.progress > 0.0;
        if t >= 1.0 {
            self.mwnd.anim.started_at = None;
            self.mwnd.anim.progress = self.mwnd.anim.to;
            self.mwnd.anim.visible = self.mwnd.anim.target_visible;
            if !self.mwnd.anim.target_visible && self.mwnd.anim.clear_text_on_close_end {
                self.mwnd.anim.clear_text_on_close_end = false;
                self.clear_message();
                self.clear_name();
            }
        }
    }

    fn resolve_mwnd_anim_type(
        &self,
        anime_type: i64,
        rect: UiRect,
        screen_w: u32,
        screen_h: u32,
    ) -> i64 {
        match anime_type {
            6 => {
                let up = rect.y + rect.h as i32;
                let down = screen_h as i32 - rect.y;
                if up <= down {
                    2
                } else {
                    3
                }
            }
            7 => {
                let left = rect.x + rect.w as i32;
                let right = screen_w as i32 - rect.x;
                if left <= right {
                    4
                } else {
                    5
                }
            }
            8 => {
                let up = rect.y + rect.h as i32;
                let down = screen_h as i32 - rect.y;
                let left = rect.x + rect.w as i32;
                let right = screen_w as i32 - rect.x;
                let (ud_ty, ud_len) = if up <= down { (2, up) } else { (3, down) };
                let (lr_ty, lr_len) = if left <= right { (4, left) } else { (5, right) };
                if ud_len <= lr_len {
                    ud_ty
                } else {
                    lr_ty
                }
            }
            _ => anime_type,
        }
    }

    fn current_window_anim(&self, rect: UiRect, screen_w: u32, screen_h: u32) -> UiWindowAnim {
        let p = self.mwnd.anim.progress.clamp(0.0, 1.0);
        let ty = self.resolve_mwnd_anim_type(self.mwnd.anim.anim_type, rect, screen_w, screen_h);
        let mut dx = 0.0f32;
        let mut dy = 0.0f32;
        let mut scale_x = 1.0f32;
        let mut scale_y = 1.0f32;
        let mut rotate_deg = 0.0f32;
        let mut alpha = if p <= 0.0 { 0.0 } else { 255.0 };
        let mut pivot_abs_x = rect.x as f32 + rect.w as f32 * 0.5;
        let mut pivot_abs_y = rect.y as f32 + rect.h as f32 * 0.5;

        let ease = p * p * (3.0 - 2.0 * p);
        let fade_alpha = |t: f32| -> f32 {
            if t <= 0.0 {
                0.0
            } else {
                (255.0 * t).clamp(0.0, 255.0)
            }
        };
        let slide_from = |start: f32, t: f32| -> f32 { start * (1.0 - t) };
        let scale_from = |start: f32, t: f32| -> f32 { start + (1.0 - start) * t };
        let resolve_anchor = |axis: char, center_code: i32| -> f32 {
            match (axis, center_code) {
                ('x', 0) => rect.x as f32 + rect.w as f32 * 0.5,
                ('x', 1) => rect.x as f32,
                ('x', 2) => rect.x as f32 + rect.w as f32,
                ('x', 3) => -(screen_w as f32) / 16.0,
                ('x', 4) => screen_w as f32 + (screen_w as f32) / 16.0,
                ('y', 0) => rect.y as f32 + rect.h as f32 * 0.5,
                ('y', 1) => rect.y as f32,
                ('y', 2) => rect.y as f32 + rect.h as f32,
                ('y', 3) => -(screen_h as f32) / 16.0,
                ('y', 4) => screen_h as f32 + (screen_h as f32) / 16.0,
                _ => 0.0,
            }
        };

        match ty {
            0 => {}
            1 => {
                alpha = fade_alpha(ease);
            }
            2 => {
                dy = slide_from(-(rect.y + rect.h as i32) as f32, ease);
                alpha = fade_alpha(ease);
            }
            3 => {
                dy = slide_from((screen_h as i32 - rect.y) as f32, ease);
                alpha = fade_alpha(ease);
            }
            4 => {
                dx = slide_from(-(rect.x + rect.w as i32) as f32, ease);
                alpha = fade_alpha(ease);
            }
            5 => {
                dx = slide_from((screen_w as i32 - rect.x) as f32, ease);
                alpha = fade_alpha(ease);
            }
            9..=48 => {
                alpha = fade_alpha(ease * (224.0 / 255.0) + p * (31.0 / 255.0));
                let (mut ud_mod, mut ud_center, mut lr_mod, mut lr_center, mut rotate_cnt) =
                    (0, 0, 0, 0, 0);
                match ty {
                    9 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                    }
                    10 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                    }
                    11 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                    }
                    12 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                    }
                    13 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                    }
                    14 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                    }
                    15 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                    }
                    16 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                    }
                    17 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 1;
                    }
                    18 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 2;
                    }
                    19 => {
                        ud_mod = 2;
                        ud_center = 1;
                        lr_mod = 0;
                        lr_center = 0;
                    }
                    20 => {
                        ud_mod = 2;
                        ud_center = 2;
                        lr_mod = 0;
                        lr_center = 0;
                    }
                    21 => {
                        ud_mod = 2;
                        ud_center = 1;
                        lr_mod = 2;
                        lr_center = 1;
                    }
                    22 => {
                        ud_mod = 2;
                        ud_center = 1;
                        lr_mod = 2;
                        lr_center = 2;
                    }
                    23 => {
                        ud_mod = 2;
                        ud_center = 2;
                        lr_mod = 2;
                        lr_center = 1;
                    }
                    24 => {
                        ud_mod = 2;
                        ud_center = 2;
                        lr_mod = 2;
                        lr_center = 2;
                    }
                    25 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 3;
                    }
                    26 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 4;
                    }
                    27 => {
                        ud_mod = 2;
                        ud_center = 3;
                        lr_mod = 0;
                        lr_center = 0;
                    }
                    28 => {
                        ud_mod = 2;
                        ud_center = 4;
                        lr_mod = 0;
                        lr_center = 0;
                    }
                    29 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = -4;
                    }
                    30 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = 4;
                    }
                    31 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = -8;
                    }
                    32 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = 8;
                    }
                    33 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                        rotate_cnt = -4;
                    }
                    34 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                        rotate_cnt = 4;
                    }
                    35 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                        rotate_cnt = -8;
                    }
                    36 => {
                        ud_mod = 1;
                        ud_center = 0;
                        lr_mod = 1;
                        lr_center = 0;
                        rotate_cnt = 8;
                    }
                    37 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                        rotate_cnt = -4;
                    }
                    38 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                        rotate_cnt = 4;
                    }
                    39 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = -4;
                    }
                    40 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = 4;
                    }
                    41 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                        rotate_cnt = -2;
                    }
                    42 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                        rotate_cnt = 2;
                    }
                    43 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = -2;
                    }
                    44 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = 2;
                    }
                    45 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                        rotate_cnt = -1;
                    }
                    46 => {
                        ud_mod = 2;
                        ud_center = 0;
                        lr_mod = 0;
                        lr_center = 0;
                        rotate_cnt = 1;
                    }
                    47 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = -1;
                    }
                    48 => {
                        ud_mod = 0;
                        ud_center = 0;
                        lr_mod = 2;
                        lr_center = 0;
                        rotate_cnt = 1;
                    }
                    _ => {}
                }
                if ud_mod != 0 {
                    pivot_abs_y = resolve_anchor('y', ud_center);
                    let start = if ud_mod == 1 { 3.0 } else { 0.0 };
                    scale_y = scale_from(start, ease);
                }
                if lr_mod != 0 {
                    pivot_abs_x = resolve_anchor('x', lr_center);
                    let start = if lr_mod == 1 { 3.0 } else { 0.0 };
                    scale_x = scale_from(start, ease);
                }
                if rotate_cnt != 0 {
                    rotate_deg = (rotate_cnt as f32 * 90.0) * (1.0 - ease);
                }
            }
            99 => {
                dx = ((1.0 - ease) * 800.0).round();
            }
            _ => {
                alpha = fade_alpha(p);
            }
        }

        UiWindowAnim {
            dx: dx.round() as i32,
            dy: dy.round() as i32,
            scale_x: scale_x.clamp(0.001, 8.0),
            scale_y: scale_y.clamp(0.001, 8.0),
            rotate: rotate_deg.to_radians(),
            alpha: alpha.round().clamp(0.0, 255.0) as u8,
            pivot_abs_x: pivot_abs_x + dx,
            pivot_abs_y: pivot_abs_y + dy,
        }
    }

    fn current_slide_offset_px(&self) -> i32 {
        if !self.mwnd.msg.slide_enabled {
            return 0;
        }
        let Some(start) = self.mwnd.msg.slide_started_at else {
            return 0;
        };
        let dur = self.mwnd.msg.slide_time_ms.max(1);
        let t = (start.elapsed().as_secs_f32() / (dur as f32 / 1000.0)).clamp(0.0, 1.0);
        ((1.0 - t) * 36.0).round() as i32
    }

    /// Ensure fixed UI sprites exist and are laid out for the given screen size.
    pub fn sync_layout(&mut self, layers: &mut crate::layer::LayerManager, w: u32, h: u32) {
        if !self.mwnd.projection_active && !self.mwnd.anim.visible {
            return;
        }
        let ui_layer = Self::ensure_layer(layers, &mut self.mwnd.layer);
        let bg_sprite = self.ensure_msg_bg_sprite(layers, ui_layer);
        let filter_sprite = self.ensure_msg_filter_sprite(layers, ui_layer);
        let face_sprite = self.ensure_msg_face_sprite(layers, ui_layer);
        let msg_text_sprite =
            Self::ensure_text_sprite(layers, ui_layer, &mut self.mwnd.msg.text_sprite);
        let name_text_sprite =
            Self::ensure_text_sprite(layers, ui_layer, &mut self.mwnd.name.text_sprite);

        let rect = self.window_rect(w, h);
        let anim = self.current_window_anim(rect, w, h);
        let apply_anim = |s: &mut crate::layer::Sprite, base_x: i32, base_y: i32, order: i32| {
            s.fit = SpriteFit::PixelRect;
            s.x = base_x + anim.dx;
            s.y = base_y + anim.dy;
            s.order = order;
            s.scale_x = anim.scale_x;
            s.scale_y = anim.scale_y;
            s.rotate = anim.rotate;
            s.pivot_x = anim.pivot_abs_x - s.x as f32;
            s.pivot_y = anim.pivot_abs_y - s.y as f32;
        };

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(bg_sprite))
        {
            s.size_mode = SpriteSizeMode::Explicit {
                width: rect.w,
                height: rect.h,
            };
            apply_anim(s, rect.x, rect.y, 1_000_000);
        }

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(filter_sprite))
        {
            s.size_mode = SpriteSizeMode::Explicit {
                width: rect.w,
                height: rect.h,
            };
            apply_anim(s, rect.x, rect.y, 1_000_005);
        }

        let face_rect = self.face_rect(w, h);
        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(face_sprite))
        {
            s.size_mode = SpriteSizeMode::Explicit {
                width: face_rect.w,
                height: face_rect.h,
            };
            apply_anim(
                s,
                face_rect.x,
                face_rect.y + self.current_slide_offset_px() / 3,
                1_000_008,
            );
        }

        let (mx, my, mw, mh) = self.msg_rect(w, h);
        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(msg_text_sprite))
        {
            s.size_mode = SpriteSizeMode::Explicit {
                width: mw,
                height: mh,
            };
            apply_anim(s, mx + self.current_slide_offset_px(), my, 1_000_010);
        }

        let (nx, ny, nw, nh) = self.name_rect(w, h);
        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(name_text_sprite))
        {
            s.size_mode = SpriteSizeMode::Explicit {
                width: nw,
                height: nh,
            };
            apply_anim(s, nx, ny, 1_000_020);
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
        self.update_message_window_anim();
        self.scan_font_dir(project_dir);
        if !self.font_cache.is_loaded() {
            let _ = self
                .font_cache
                .load_from_font_dir(&project_dir.join("font"));
        }
        self.refresh_waku_images(images, project_dir);
        self.refresh_face_image(images, project_dir);
        self.sync_layout(layers, w, h);
        self.update_message_reveal(script, syscom);
        self.refresh_text_images(images, w, h);
        self.sync_sys_overlay(layers, images, w, h);

        let Some(ui_layer) = self.mwnd.layer else {
            return;
        };
        let Some(bg_sprite) = self.mwnd.waku.bg_sprite else {
            return;
        };
        let anim_alpha = self.current_window_anim(self.window_rect(w, h), w, h).alpha;

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(bg_sprite))
        {
            s.visible = self.mwnd.anim.visible && self.mwnd.waku.bg_image.is_some();
            s.alpha = anim_alpha;
            s.image_id = self.mwnd.waku.bg_image;
        }

        if let Some(sprite_id) = self.mwnd.waku.filter_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                let visible = self.mwnd.anim.visible && self.mwnd.waku.filter_image.is_some();
                s.visible = visible;
                s.image_id = self.mwnd.waku.filter_image;
                s.alpha = anim_alpha;

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

                s.alpha = ((a as u16 * anim_alpha as u16) / 255) as u8;
                s.tr = 255;
                s.color_rate = 255;
                s.color_r = r;
                s.color_g = g;
                s.color_b = b;
                s.mask_mode = 1;
            }
        }

        if let Some(sprite_id) = self.mwnd.face.sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = self.mwnd.anim.visible && self.mwnd.face.image.is_some();
                s.image_id = self.mwnd.face.image;
                s.alpha = anim_alpha;
            }
        }

        if let Some(sprite_id) = self.mwnd.msg.text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = self.mwnd.anim.visible && self.mwnd.msg.text_image.is_some();
                s.image_id = self.mwnd.msg.text_image;
                s.alpha = anim_alpha;
            }
        }

        if let Some(sprite_id) = self.mwnd.name.text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = self.mwnd.anim.visible && self.mwnd.name.text_image.is_some();
                s.image_id = self.mwnd.name.text_image;
                s.alpha = anim_alpha;
            }
        }

        if let Some(sys_bg) = self.sys.bg_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sys_bg))
            {
                s.visible = self.sys.active;
                if let Some(img) = self.sys.bg_image {
                    s.image_id = Some(img);
                }
            }
        }
        if let Some(sys_text) = self.sys.text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sys_text))
            {
                s.visible = self.sys.active && self.sys.text_image.is_some();
                s.image_id = self.sys.text_image;
            }
        }
    }

    pub fn set_message_bg(&mut self, img: ImageId) {
        self.mwnd.projection_active = true;
        self.mwnd.waku.bg_image = Some(img);
    }

    pub fn show_message_bg(&mut self, on: bool) {
        self.mwnd.anim.target_visible = on;
        if self.mwnd.anim.started_at.is_none() {
            self.mwnd.anim.visible = on;
            self.mwnd.anim.progress = if on { 1.0 } else { 0.0 };
            self.mwnd.anim.from = self.mwnd.anim.progress;
            self.mwnd.anim.to = self.mwnd.anim.progress;
        }
    }

    pub fn force_message_bg_visible(&mut self, on: bool) {
        self.mwnd.anim.target_visible = on;
        self.mwnd.anim.visible = on;
        self.mwnd.anim.progress = if on { 1.0 } else { 0.0 };
        self.mwnd.anim.from = self.mwnd.anim.progress;
        self.mwnd.anim.to = self.mwnd.anim.progress;
        self.mwnd.anim.started_at = None;
        self.mwnd.anim.duration_ms = 0;
        self.mwnd.anim.anim_type = 0;
        self.mwnd.anim.clear_text_on_close_end = false;
        if !on {
            self.clear_message();
            self.clear_name();
        }
    }

    pub fn begin_mwnd_open(&mut self, anime_type: i64, duration_ms: i64) {
        self.begin_message_window_anim(true, anime_type, duration_ms.max(0) as u64, false);
    }

    pub fn begin_mwnd_close(&mut self, anime_type: i64, duration_ms: i64) {
        self.begin_message_window_anim(false, anime_type, duration_ms.max(0) as u64, true);
    }

    pub fn set_message_filter(&mut self, img: Option<ImageId>) {
        self.mwnd.waku.filter_image = img;
    }

    pub fn apply_mwnd_projection(&mut self, proj: &MwndProjectionState) {
        self.mwnd.projection_active = true;

        let bg_file = proj
            .bg_file
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if self.mwnd.waku.bg_file != bg_file {
            self.mwnd.waku.bg_file = bg_file;
            self.mwnd.waku.bg_image = None;
            self.mwnd.waku.bg_size = None;
        }

        let filter_file = proj
            .filter_file
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if self.mwnd.waku.filter_file != filter_file {
            self.mwnd.waku.filter_file = filter_file;
            self.mwnd.waku.filter_image = None;
        }

        self.set_mwnd_window_state(
            proj.window_pos,
            proj.window_size,
            proj.window_moji_cnt,
            proj.moji_size,
            proj.moji_color,
            proj.face_file.as_deref(),
            proj.face_no,
            proj.rep_pos,
            proj.slide_enabled,
            proj.slide_time,
        );
        self.set_name(proj.name_text.clone());
        if proj.msg_text.is_empty() {
            if !(self.mwnd.msg.waiting && self.mwnd.msg.clear_on_wait_end) {
                self.clear_message();
            }
        } else {
            self.set_message(proj.msg_text.clone());
        }
    }

    pub fn set_mwnd_window_state(
        &mut self,
        window_pos: Option<(i64, i64)>,
        window_size: Option<(i64, i64)>,
        window_moji_cnt: Option<(i64, i64)>,
        moji_size: Option<i64>,
        moji_color: Option<i64>,
        face_file: Option<&str>,
        face_no: i64,
        rep_pos: Option<(i64, i64)>,
        slide_enabled: bool,
        slide_time: i64,
    ) {
        self.mwnd.window.pos = window_pos.map(|(x, y)| (x as i32, y as i32));
        self.mwnd.window.size = window_size.map(|(w, h)| (w.max(1) as u32, h.max(1) as u32));
        self.mwnd.window.moji_cnt = window_moji_cnt;
        self.mwnd.window.moji_size = moji_size;
        self.mwnd.window.moji_color = moji_color;
        self.mwnd.face.rep_pos = rep_pos;
        self.mwnd.msg.slide_enabled = slide_enabled;
        self.mwnd.msg.slide_time_ms = slide_time.max(0) as u64;
        let new_face = face_file.filter(|s| !s.is_empty()).map(str::to_string);
        if self.mwnd.face.file != new_face || self.mwnd.face.no != face_no {
            self.mwnd.face.file = new_face;
            self.mwnd.face.no = face_no;
            self.mwnd.face.image = None;
        }
        self.mwnd.msg.text_dirty = true;
        self.mwnd.name.text_dirty = true;
    }

    pub fn clear_mwnd_window_state(&mut self) {
        self.mwnd.window.pos = None;
        self.mwnd.window.size = None;
        self.mwnd.window.moji_cnt = None;
        self.mwnd.window.moji_size = None;
        self.mwnd.window.moji_color = None;
        self.mwnd.waku.bg_file = None;
        self.mwnd.waku.filter_file = None;
        self.mwnd.projection_active = false;
        self.mwnd.waku.bg_image = None;
        self.mwnd.waku.filter_image = None;
        self.mwnd.waku.bg_size = None;
        self.mwnd.face.file = None;
        self.mwnd.face.no = 0;
        self.mwnd.face.rep_pos = None;
        self.mwnd.face.image = None;
        self.mwnd.msg.slide_enabled = false;
        self.mwnd.msg.slide_time_ms = 0;
        self.mwnd.msg.slide_started_at = None;
        self.mwnd.anim.anim_type = 0;
        self.mwnd.msg.text_dirty = true;
        self.mwnd.name.text_dirty = true;
    }

    pub fn set_message(&mut self, msg: String) {
        let new_text = if msg.is_empty() { None } else { Some(msg) };
        if self.mwnd.msg.text == new_text {
            return;
        }
        self.mwnd.msg.text = new_text;
        self.mwnd.msg.text_dirty = true;
        self.mwnd.msg.visible_chars = 0;
        self.mwnd.msg.reveal_base = 0;
        self.mwnd.msg.reveal_start = Some(Instant::now());
        if self.mwnd.msg.slide_enabled {
            self.mwnd.msg.slide_started_at = Some(Instant::now());
        }
    }

    pub fn append_message(&mut self, msg: &str) {
        if msg.is_empty() {
            return;
        }
        match self.mwnd.msg.text.as_mut() {
            Some(s) => s.push_str(msg),
            None => self.mwnd.msg.text = Some(msg.to_string()),
        }
        self.mwnd.msg.text_dirty = true;
        self.mwnd.msg.reveal_base = self.mwnd.msg.visible_chars;
        self.mwnd.msg.reveal_start = Some(Instant::now());
        if self.mwnd.msg.slide_enabled {
            self.mwnd.msg.slide_started_at = Some(Instant::now());
        }
    }

    pub fn append_linebreak(&mut self) {
        match self.mwnd.msg.text.as_mut() {
            Some(s) => s.push('\n'),
            None => self.mwnd.msg.text = Some("\n".to_string()),
        }
        self.mwnd.msg.text_dirty = true;
        self.mwnd.msg.reveal_base = self.mwnd.msg.visible_chars;
        self.mwnd.msg.reveal_start = Some(Instant::now());
        if self.mwnd.msg.slide_enabled {
            self.mwnd.msg.slide_started_at = Some(Instant::now());
        }
    }

    pub fn set_name(&mut self, name: String) {
        let new_text = if name.is_empty() { None } else { Some(name) };
        if self.mwnd.name.text == new_text {
            return;
        }
        self.mwnd.name.text = new_text;
        self.mwnd.name.text_dirty = true;
    }

    pub fn clear_name(&mut self) {
        if self.mwnd.name.text.is_none() {
            return;
        }
        self.mwnd.name.text = None;
        self.mwnd.name.text_dirty = true;
    }

    pub fn clear_message(&mut self) {
        if self.mwnd.msg.text.is_none() {
            return;
        }
        self.mwnd.msg.text = None;
        self.mwnd.msg.text_dirty = true;
        self.mwnd.msg.visible_chars = 0;
        self.mwnd.msg.reveal_base = 0;
        self.mwnd.msg.reveal_start = None;
        self.mwnd.msg.slide_started_at = None;
    }

    pub fn begin_wait_message(&mut self) {
        self.mwnd.msg.waiting = true;
        self.mwnd.msg.wait_started_at = Some(Instant::now());
        self.mwnd.msg.wait_message_len =
            self.mwnd.msg.text.as_deref().unwrap_or("").chars().count();
    }

    pub fn end_wait_message(&mut self) {
        self.mwnd.msg.waiting = false;
        self.mwnd.msg.wait_started_at = None;

        if self.mwnd.msg.clear_on_wait_end {
            self.mwnd.msg.clear_on_wait_end = false;
            self.clear_message();
        }
    }

    pub fn request_clear_message_on_wait_end(&mut self) {
        self.mwnd.msg.clear_on_wait_end = true;
    }

    pub fn set_sys_overlay(&mut self, active: bool, text: String) {
        self.sys.active = active;
        if self.sys.text != text {
            self.sys.text = text;
            self.sys.text_dirty = true;
        }
    }

    pub fn message_text(&self) -> Option<&str> {
        self.mwnd.msg.text.as_deref()
    }

    pub fn name_text(&self) -> Option<&str> {
        self.mwnd.name.text.as_deref()
    }

    pub fn auto_advance_due(
        &self,
        script: &ScriptRuntimeState,
        syscom: &SyscomRuntimeState,
    ) -> bool {
        if !self.mwnd.msg.waiting {
            return false;
        }
        if script.msg_nowait {
            return true;
        }
        let auto_mode = script.auto_mode_flag || syscom.auto_mode.onoff;
        if !auto_mode {
            return false;
        }
        let Some(start) = self.mwnd.msg.wait_started_at else {
            return false;
        };
        let (moji_wait, min_wait) = auto_mode_timing(script, syscom);
        let len = self.mwnd.msg.wait_message_len.max(1) as i64;
        let by_len = moji_wait.saturating_mul(len);
        let total = by_len.max(min_wait).max(0) as u64;
        start.elapsed() >= Duration::from_millis(total)
    }

    fn update_message_reveal(&mut self, script: &ScriptRuntimeState, syscom: &SyscomRuntimeState) {
        let total = self.mwnd.msg.text.as_deref().unwrap_or("").chars().count();
        if total == 0 {
            self.mwnd.msg.visible_chars = 0;
            self.mwnd.msg.reveal_base = 0;
            self.mwnd.msg.reveal_start = None;
            return;
        }

        if script.msg_nowait {
            if self.mwnd.msg.visible_chars != total {
                self.mwnd.msg.visible_chars = total;
                self.mwnd.msg.text_dirty = true;
            }
            self.mwnd.msg.reveal_base = total;
            self.mwnd.msg.reveal_start = None;
            return;
        }

        let Some(ms_per_char) = message_speed_ms(script, syscom) else {
            if self.mwnd.msg.visible_chars != total {
                self.mwnd.msg.visible_chars = total;
                self.mwnd.msg.text_dirty = true;
            }
            self.mwnd.msg.reveal_base = total;
            self.mwnd.msg.reveal_start = None;
            return;
        };

        let Some(start) = self.mwnd.msg.reveal_start else {
            return;
        };
        let elapsed = start.elapsed().as_millis() as usize;
        let inc = if ms_per_char == 0 {
            total
        } else {
            elapsed / ms_per_char as usize
        };
        let visible = self.mwnd.msg.reveal_base.saturating_add(inc).min(total);
        if self.mwnd.msg.visible_chars != visible {
            self.mwnd.msg.visible_chars = visible;
            self.mwnd.msg.text_dirty = true;
        }
        if visible >= total {
            self.mwnd.msg.reveal_base = total;
            self.mwnd.msg.reveal_start = None;
        }
    }

    fn visible_message_text(&self) -> String {
        let Some(msg) = self.mwnd.msg.text.as_deref() else {
            return String::new();
        };
        if self.mwnd.msg.visible_chars == 0 {
            return String::new();
        }
        msg.chars().take(self.mwnd.msg.visible_chars).collect()
    }

    fn refresh_waku_images(
        &mut self,
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
    ) {
        if let Some(id) = self.mwnd.waku.bg_image {
            if self.mwnd.waku.bg_size.is_none() {
                if let Some(img) = images.get(id) {
                    self.mwnd.waku.bg_size = Some((img.width, img.height));
                }
            }
        }
        if self.mwnd.waku.bg_image.is_none() {
            if let Some(raw) = self.mwnd.waku.bg_file.as_deref() {
                if !raw.is_empty() {
                    let path = project_dir.join(raw);
                    if let Ok(id) = images.load_file(Path::new(raw), 0) {
                        self.mwnd.waku.bg_image = Some(id);
                    } else if let Ok(id) = images.load_file(&path, 0) {
                        self.mwnd.waku.bg_image = Some(id);
                    } else if let Ok(id) = images.load_g00(raw, 0) {
                        self.mwnd.waku.bg_image = Some(id);
                    } else if let Ok(id) = images.load_bg(raw) {
                        self.mwnd.waku.bg_image = Some(id);
                    }
                    if let Some(id) = self.mwnd.waku.bg_image {
                        if let Some(img) = images.get(id) {
                            self.mwnd.waku.bg_size = Some((img.width, img.height));
                        }
                    }
                }
            }
        }

        if self.mwnd.waku.filter_image.is_none() {
            if let Some(raw) = self.mwnd.waku.filter_file.as_deref() {
                if !raw.is_empty() {
                    let path = project_dir.join(raw);
                    if let Ok(id) = images.load_file(Path::new(raw), 0) {
                        self.mwnd.waku.filter_image = Some(id);
                    } else if let Ok(id) = images.load_file(&path, 0) {
                        self.mwnd.waku.filter_image = Some(id);
                    }
                }
            }
        }
    }

    fn refresh_face_image(
        &mut self,
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
    ) {
        let Some(raw) = self.mwnd.face.file.as_deref() else {
            self.mwnd.face.image = None;
            return;
        };
        if raw.is_empty() {
            self.mwnd.face.image = None;
            return;
        }
        if self.mwnd.face.image.is_some() {
            return;
        }
        let pat = self.mwnd.face.no.max(0) as u32;
        if let Ok(id) = images.load_g00(raw, pat) {
            self.mwnd.face.image = Some(id);
            return;
        }
        if let Ok(id) = images.load_bg(raw) {
            self.mwnd.face.image = Some(id);
            return;
        }
        let path = project_dir.join(raw);
        if path.exists() {
            if let Ok(id) = images.load_file(&path, 0) {
                self.mwnd.face.image = Some(id);
            }
        }
    }

    fn refresh_text_images(
        &mut self,
        images: &mut crate::image_manager::ImageManager,
        w: u32,
        h: u32,
    ) {
        if self.mwnd.msg.text_dirty {
            let (x, y, mw, mh) = self.msg_rect(w, h);
            let _ = (x, y);
            let font_size = self.message_font_px() as f32;
            self.mwnd.msg.text_image = self.font_cache.render_text(
                images,
                &self.visible_message_text(),
                font_size,
                mw,
                mh,
            );
            self.mwnd.msg.text_dirty = false;
        }

        if self.mwnd.name.text_dirty {
            let (x, y, mw, mh) = self.name_rect(w, h);
            let _ = (x, y);
            let font_size = self.name_font_px() as f32;
            self.mwnd.name.text_image = self.font_cache.render_text(
                images,
                self.mwnd.name.text.as_deref().unwrap_or(""),
                font_size,
                mw,
                mh,
            );
            self.mwnd.name.text_dirty = false;
        }
    }

    fn sync_sys_overlay(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        images: &mut crate::image_manager::ImageManager,
        w: u32,
        h: u32,
    ) {
        if !self.sys.active {
            return;
        }
        let ui_layer = Self::ensure_layer(layers, &mut self.mwnd.layer);
        let bg = self.ensure_sys_bg_sprite(layers, ui_layer);
        let text = Self::ensure_text_sprite(layers, ui_layer, &mut self.sys.text_sprite);

        if self.sys.bg_image.is_none() {
            self.sys.bg_image = Some(images.solid_rgba((0, 0, 0, 180)));
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

        if self.sys.text_dirty {
            self.sys.text_image = self.font_cache.render_text(
                images,
                &self.sys.text,
                24.0,
                w.saturating_sub(80),
                h.saturating_sub(80),
            );
            self.sys.text_dirty = false;
        }
    }

    fn ensure_sys_bg_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.sys.bg_sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.sys.bg_sprite = Some(sprite_id);
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
