//! Message-window rendering state projected from runtime MWND state.

use crate::image_manager::ImageId;
use crate::layer::{LayerId, Sprite, SpriteFit, SpriteId, SpriteSizeMode};
use crate::runtime::globals::{EditBoxListState, ScriptRuntimeState, SyscomRuntimeState};
use crate::text_render::{FontCache, TextStyle};
use std::collections::HashMap;
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

#[derive(Debug, Clone, Copy)]
pub struct MwndWindowRenderState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub dx: i32,
    pub dy: i32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotate: f32,
    pub alpha: u8,
    pub pivot_abs_x: f32,
    pub pivot_abs_y: f32,
}

/// UI-side sprite cache for the message-window family and related overlays.
#[derive(Debug, Default)]
pub struct MwndWakuRuntime {
    pub bg_sprite: Option<SpriteId>,
    pub filter_sprite: Option<SpriteId>,
    pub bg_image: Option<ImageId>,
    pub filter_image: Option<ImageId>,
    pub solid_filter_image: Option<ImageId>,
    pub bg_file: Option<String>,
    pub filter_file: Option<String>,
    pub bg_size: Option<(u32, u32)>,
    pub filter_size: Option<(u32, u32)>,
    pub filter_margin: (i64, i64, i64, i64),
    pub filter_color: (u8, u8, u8, u8),
    pub filter_config_color: bool,
    pub filter_config_tr: bool,
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
pub struct MwndKeyIconRuntime {
    pub sprite: Option<SpriteId>,
    pub image: Option<ImageId>,
    pub file: Option<String>,
    pub cached_mode: i64,
    pub cached_pat: i64,
    pub size: Option<(u32, u32)>,
    pub key_file: Option<String>,
    pub key_pat_cnt: i64,
    pub key_speed: i64,
    pub page_file: Option<String>,
    pub page_pat_cnt: i64,
    pub page_speed: i64,
    pub appear: bool,
    pub mode: i64,
    pub anime_start: Option<Instant>,
    pub icon_pos_type: i64,
    pub icon_pos_base: i64,
    pub icon_pos: (i64, i64, i64),
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
    pub message_pos: Option<(i32, i32)>,
    pub message_margin: Option<(i64, i64, i64, i64)>,
    pub moji_cnt: Option<(i64, i64)>,
    pub moji_size: Option<i64>,
    pub moji_space: Option<(i64, i64)>,
    pub extend_type: i64,
    pub moji_color: Option<i64>,
    pub shadow_color: Option<i64>,
    pub fuchi_color: Option<i64>,
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
    pub key_icon: MwndKeyIconRuntime,
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

#[derive(Debug, Clone)]
pub struct MsgBackTextProjection {
    pub history_index: usize,
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub style: TextStyle,
}

#[derive(Debug, Clone)]
pub struct MsgBackImageProjection {
    pub file: Option<String>,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct MsgBackEntryButtonProjection {
    pub history_index: usize,
    pub file: Option<String>,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct MsgBackUiProjection {
    pub window_x: i32,
    pub window_y: i32,
    pub window_w: u32,
    pub window_h: u32,
    pub disp_margin: (i64, i64, i64, i64),
    pub msg_pos: i32,
    pub moji_size: i64,
    pub moji_space: Option<(i64, i64)>,
    pub order: i32,
    pub filter_layer_rep: i32,
    pub waku_layer_rep: i32,
    pub moji_layer_rep: i32,
    pub waku_file: Option<String>,
    pub filter_file: Option<String>,
    pub filter_margin: (i64, i64, i64, i64),
    /// MSGBK.FILTER_COLOR, used when no filter texture is configured.
    pub filter_rgba: (u8, u8, u8, u8),
    /// Runtime CONFIG.FILTER_COLOR/Gp_config->filter_color, applied to the filter sprite.
    pub filter_config_rgba: (u8, u8, u8, u8),
    pub text_entries: Vec<MsgBackTextProjection>,
    pub separators: Vec<MsgBackImageProjection>,
    pub koe_buttons: Vec<MsgBackEntryButtonProjection>,
    pub load_buttons: Vec<MsgBackEntryButtonProjection>,
    pub close_btn_file: Option<String>,
    pub close_btn_pos: (i32, i32),
    pub msg_up_btn_file: Option<String>,
    pub msg_up_btn_pos: (i32, i32),
    pub msg_down_btn_file: Option<String>,
    pub msg_down_btn_pos: (i32, i32),
    pub slider_file: Option<String>,
    pub slider_rect: (i32, i32, i32, i32),
    pub slider_pos: (i32, i32),
    pub ex_btn_files: [Option<String>; 4],
    pub ex_btn_pos: [(i32, i32); 4],
}


fn msg_back_packed_sorter_key(order: i32, layer: i32) -> i32 {
    let packed = (order as i64)
        .clamp(i32::MIN as i64 / 1024, i32::MAX as i64 / 1024)
        .saturating_mul(1024)
        .saturating_add((layer as i64).clamp(-1023, 1023));
    packed as i32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgBackHitAction {
    Close,
    Up,
    Down,
    Slider,
}

#[derive(Debug, Default)]
pub struct MsgBackButtonRuntime {
    pub sprite: Option<SpriteId>,
    pub image: Option<ImageId>,
    pub cached_file: Option<String>,
    pub size: Option<(u32, u32)>,
    pub center: Option<(i32, i32)>,
}

#[derive(Debug, Default)]
pub struct MsgBackTextRuntime {
    pub sprite: Option<SpriteId>,
    pub image: Option<ImageId>,
}

#[derive(Debug, Default)]
pub struct MsgBackRuntime {
    pub projection: Option<MsgBackUiProjection>,
    pub waku_sprite: Option<SpriteId>,
    pub filter_sprite: Option<SpriteId>,
    pub text_sprite: Option<SpriteId>,
    pub waku_image: Option<ImageId>,
    pub filter_image: Option<ImageId>,
    pub solid_filter_image: Option<ImageId>,
    pub solid_filter_color: Option<(u8, u8, u8, u8)>,
    pub text_image: Option<ImageId>,
    pub cached_waku_file: Option<String>,
    pub cached_filter_file: Option<String>,
    pub text_dirty: bool,
    pub text_entries: Vec<MsgBackTextRuntime>,
    pub separators: Vec<MsgBackButtonRuntime>,
    pub koe_buttons: Vec<MsgBackButtonRuntime>,
    pub load_buttons: Vec<MsgBackButtonRuntime>,
    pub close_btn: MsgBackButtonRuntime,
    pub msg_up_btn: MsgBackButtonRuntime,
    pub msg_down_btn: MsgBackButtonRuntime,
    pub slider: MsgBackButtonRuntime,
    pub ex_buttons: Vec<MsgBackButtonRuntime>,
}

#[derive(Debug, Default)]
pub struct EditBoxOverlayEntry {
    pub bg_sprite: Option<SpriteId>,
    pub text_sprite: Option<SpriteId>,
    pub text_image: Option<ImageId>,
    pub last_text: String,
    pub last_w: u32,
    pub last_h: u32,
    pub last_font_px: u32,
    pub last_focused: bool,
}

#[derive(Debug, Default)]
pub struct EditBoxOverlayRuntime {
    pub layer: Option<LayerId>,
    pub bg_image: Option<ImageId>,
    pub focused_bg_image: Option<ImageId>,
    pub entries: HashMap<(u32, usize), EditBoxOverlayEntry>,
}

#[derive(Debug, Default, Clone)]
pub struct MwndProjectionState {
    pub bg_file: Option<String>,
    pub filter_file: Option<String>,
    pub filter_margin: Option<(i64, i64, i64, i64)>,
    pub filter_color: Option<(u8, u8, u8, u8)>,
    pub filter_config_color: bool,
    pub filter_config_tr: bool,
    pub face_file: Option<String>,
    pub face_no: i64,
    pub rep_pos: Option<(i64, i64)>,
    pub window_pos: Option<(i64, i64)>,
    pub window_size: Option<(i64, i64)>,
    pub message_pos: Option<(i64, i64)>,
    pub message_margin: Option<(i64, i64, i64, i64)>,
    pub window_moji_cnt: Option<(i64, i64)>,
    pub moji_size: Option<i64>,
    pub moji_space: Option<(i64, i64)>,
    pub mwnd_extend_type: i64,
    pub moji_color: Option<i64>,
    pub shadow_color: Option<i64>,
    pub fuchi_color: Option<i64>,
    pub chara_moji_color: Option<i64>,
    pub chara_shadow_color: Option<i64>,
    pub chara_fuchi_color: Option<i64>,
    pub name_moji_color: Option<i64>,
    pub name_shadow_color: Option<i64>,
    pub name_fuchi_color: Option<i64>,
    pub key_icon_file: Option<String>,
    pub key_icon_pat_cnt: i64,
    pub key_icon_speed: i64,
    pub page_icon_file: Option<String>,
    pub page_icon_pat_cnt: i64,
    pub page_icon_speed: i64,
    pub key_icon_appear: bool,
    pub key_icon_mode: i64,
    pub key_icon_pos: Option<(i64, i64)>,
    pub icon_pos_type: i64,
    pub icon_pos_base: i64,
    pub icon_pos: Option<(i64, i64, i64)>,
    pub slide_enabled: bool,
    pub slide_time: i64,
    pub name_text: String,
    pub msg_text: String,
}

#[derive(Debug, Default)]
pub struct UiRuntime {
    pub mwnd: MwndRuntime,
    pub sys: SysOverlayRuntime,
    pub msg_back: MsgBackRuntime,
    pub editbox: EditBoxOverlayRuntime,
    text_color: (u8, u8, u8),
    shadow_color: (u8, u8, u8),
    fuchi_color: (u8, u8, u8),
    fuchi_enabled: bool,
    name_text_color: (u8, u8, u8),
    name_shadow_color: (u8, u8, u8),
    name_fuchi_color: (u8, u8, u8),
    name_fuchi_enabled: bool,
    font_paths: Vec<PathBuf>,
    font_scanned: bool,
    font_cache: FontCache,
}

impl UiRuntime {
    pub fn set_text_colors(&mut self, text_color: (u8, u8, u8), shadow_color: (u8, u8, u8)) {
        self.set_text_colors_full(text_color, shadow_color, None);
    }

    pub fn set_text_colors_full(
        &mut self,
        text_color: (u8, u8, u8),
        shadow_color: (u8, u8, u8),
        fuchi_color: Option<(u8, u8, u8)>,
    ) {
        self.set_mwnd_text_colors_full(
            text_color,
            shadow_color,
            fuchi_color,
            text_color,
            shadow_color,
            fuchi_color,
        );
    }

    pub fn set_mwnd_text_colors_full(
        &mut self,
        msg_text_color: (u8, u8, u8),
        msg_shadow_color: (u8, u8, u8),
        msg_fuchi_color: Option<(u8, u8, u8)>,
        name_text_color: (u8, u8, u8),
        name_shadow_color: (u8, u8, u8),
        name_fuchi_color: Option<(u8, u8, u8)>,
    ) {
        self.text_color = msg_text_color;
        self.shadow_color = msg_shadow_color;
        self.fuchi_enabled = msg_fuchi_color.is_some();
        if let Some(color) = msg_fuchi_color {
            self.fuchi_color = color;
        }
        self.name_text_color = name_text_color;
        self.name_shadow_color = name_shadow_color;
        self.name_fuchi_enabled = name_fuchi_color.is_some();
        if let Some(color) = name_fuchi_color {
            self.name_fuchi_color = color;
        }
        self.mwnd.msg.text_dirty = true;
        self.mwnd.name.text_dirty = true;
    }

    fn mwnd_message_text_style(&self, script: &ScriptRuntimeState) -> TextStyle {
        TextStyle {
            color: self.text_color,
            shadow_color: self.shadow_color,
            fuchi_color: self.fuchi_color,
            shadow: script.font_shadow != 0,
            fuchi: self.fuchi_enabled,
            bold: script.font_bold != 0,
        }
    }

    fn mwnd_name_text_style(&self, script: &ScriptRuntimeState) -> TextStyle {
        TextStyle {
            color: self.name_text_color,
            shadow_color: self.name_shadow_color,
            fuchi_color: self.name_fuchi_color,
            shadow: script.font_shadow != 0,
            fuchi: self.name_fuchi_enabled,
            bold: script.font_bold != 0,
        }
    }

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

    fn ensure_key_icon_sprite(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
    ) -> SpriteId {
        if let Some(id) = self.mwnd.key_icon.sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        self.mwnd.key_icon.sprite = Some(sprite_id);
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
        if self.mwnd.window.extend_type == 1 {
            let (l, t, r, b) = self.mwnd.window.message_margin.unwrap_or((20, 20, 20, 20));
            let x = rect.x + l as i32;
            let y = rect.y + t as i32;
            let width = (rect.w as i32 - l as i32 - r as i32).max(1) as u32;
            let height = (rect.h as i32 - t as i32 - b as i32).max(1) as u32;
            return (x, y, width, height);
        }

        let face_pad = if self.mwnd.face.file.is_some() || self.mwnd.face.image.is_some() {
            self.face_reserved_width(rect) + pad / 2
        } else {
            0
        };
        let fallback_x = rect.x + pad + face_pad;
        let fallback_y = rect.y + pad + name_h;
        let (x, y) = if let Some((mx, my)) = self.mwnd.window.message_pos {
            (rect.x + mx, rect.y + my)
        } else {
            (fallback_x, fallback_y)
        };

        if let Some((cols, rows)) = self.mwnd.window.moji_cnt {
            let font_px = self.message_font_px() as i32;
            let (space_x, space_y) = self.mwnd.window.moji_space.unwrap_or((-1, 10));
            let cols = cols.max(1) as i32;
            let rows = rows.max(1) as i32;
            let width = (font_px * cols + space_x as i32 * (cols - 1)).max(1) as u32;
            let height = (font_px * rows + space_y as i32 * (rows - 1)).max(font_px) as u32;
            return (x, y, width, height);
        }

        let (l, t, r, b) = self
            .mwnd
            .window
            .message_margin
            .unwrap_or((pad as i64, pad as i64, pad as i64, pad as i64));
        let right_pad = if self.mwnd.window.message_pos.is_some() {
            r as i32
        } else {
            l as i32
        };
        let bottom_pad = if self.mwnd.window.message_pos.is_some() {
            b as i32
        } else {
            t as i32
        };
        let width = (rect.x + rect.w as i32 - x - right_pad).max(1) as u32;
        let height = (rect.y + rect.h as i32 - y - bottom_pad).max(1) as u32;
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

    fn key_icon_rect(&self, w: u32, h: u32) -> Option<UiRect> {
        let rect = self.window_rect(w, h);
        let (iw, ih) = self.mwnd.key_icon.size?;
        let (ix, iy, _iz) = self.mwnd.key_icon.icon_pos;
        let x;
        let y;
        if self.mwnd.key_icon.icon_pos_type == 0 {
            match self.mwnd.key_icon.icon_pos_base {
                1 => {
                    x = rect.x + rect.w as i32 - ix as i32 - iw as i32;
                    y = rect.y + iy as i32;
                }
                2 => {
                    x = rect.x + ix as i32;
                    y = rect.y + rect.h as i32 - iy as i32 - ih as i32;
                }
                3 => {
                    x = rect.x + rect.w as i32 - ix as i32 - iw as i32;
                    y = rect.y + rect.h as i32 - iy as i32 - ih as i32;
                }
                _ => {
                    x = rect.x + ix as i32;
                    y = rect.y + iy as i32;
                }
            }
        } else {
            x = rect.x + ix as i32;
            y = rect.y + iy as i32;
        }
        Some(UiRect::new(x, y, iw, ih))
    }

    fn message_has_text(&self) -> bool {
        self.mwnd
            .msg
            .text
            .as_deref()
            .unwrap_or("")
            .chars()
            .next()
            .is_some()
    }

    fn message_fully_revealed(&self) -> bool {
        let total = self.mwnd.msg.text.as_deref().unwrap_or("").chars().count();
        total == 0 || self.mwnd.msg.visible_chars >= total
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

    pub fn current_mwnd_window_render_state(
        &self,
        screen_w: u32,
        screen_h: u32,
    ) -> Option<MwndWindowRenderState> {
        if !self.mwnd.projection_active && !self.mwnd.anim.visible {
            return None;
        }
        let rect = self.window_rect(screen_w, screen_h);
        let anim = self.current_window_anim(rect, screen_w, screen_h);
        Some(MwndWindowRenderState {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            dx: anim.dx,
            dy: anim.dy,
            scale_x: anim.scale_x,
            scale_y: anim.scale_y,
            rotate: anim.rotate,
            alpha: anim.alpha,
            pivot_abs_x: anim.pivot_abs_x,
            pivot_abs_y: anim.pivot_abs_y,
        })
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
        let key_icon_sprite = self.ensure_key_icon_sprite(layers, ui_layer);
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
            let (ml, mt, mr, mb) = self.mwnd.waku.filter_margin;
            let fx = rect.x + ml as i32;
            let fy = rect.y + mt as i32;
            if self.mwnd.waku.filter_image.is_some() {
                s.size_mode = SpriteSizeMode::Intrinsic;
            } else {
                let width = (rect.w as i64 - ml - mr).max(1) as u32;
                let height = (rect.h as i64 - mt - mb).max(1) as u32;
                s.size_mode = SpriteSizeMode::Explicit { width, height };
            }
            apply_anim(s, fx, fy, 1_000_005);
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
            s.size_mode = if self.mwnd.msg.text_image.is_some() {
                SpriteSizeMode::Intrinsic
            } else {
                SpriteSizeMode::Explicit {
                    width: mw,
                    height: mh,
                }
            };
            apply_anim(s, mx + self.current_slide_offset_px(), my, 1_000_010);
        }

        let (nx, ny, nw, nh) = self.name_rect(w, h);
        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(name_text_sprite))
        {
            s.size_mode = if self.mwnd.name.text_image.is_some() {
                SpriteSizeMode::Intrinsic
            } else {
                SpriteSizeMode::Explicit {
                    width: nw,
                    height: nh,
                }
            };
            apply_anim(s, nx, ny, 1_000_020);
        }

        if let Some(icon_rect) = self.key_icon_rect(w, h) {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(key_icon_sprite))
            {
                s.size_mode = SpriteSizeMode::Intrinsic;
                apply_anim(s, icon_rect.x, icon_rect.y, 1_000_030);
            }
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
        editbox_lists: &HashMap<u32, EditBoxListState>,
        focused_editbox: Option<(u32, usize)>,
    ) {
        self.update_message_window_anim();
        self.scan_font_dir(project_dir);
        if !self.font_cache.is_loaded() {
            let _ = self.font_cache.load_for_project(project_dir);
        }
        self.refresh_waku_images(images, project_dir);
        self.refresh_face_image(images, project_dir);
        self.refresh_key_icon_image(images, project_dir);
        self.sync_layout(layers, w, h);
        self.update_message_reveal(script, syscom);
        self.refresh_text_images(images, w, h, script);
        self.sync_sys_overlay(layers, images, w, h);
        self.sync_msg_back_ui(layers, images, project_dir);
        self.sync_editbox_overlay(layers, images, editbox_lists, focused_editbox);

        let Some(ui_layer) = self.mwnd.layer else {
            return;
        };
        let Some(bg_sprite) = self.mwnd.waku.bg_sprite else {
            return;
        };
        let mwnd_hidden = script.mwnd_disp_off_flag || syscom.hide_mwnd.onoff || syscom.msg_back_open;
        let mwnd_visible = self.mwnd.anim.visible && !mwnd_hidden;
        let anim_alpha = self.current_window_anim(self.window_rect(w, h), w, h).alpha;

        if let Some(s) = layers
            .layer_mut(ui_layer)
            .and_then(|l| l.sprite_mut(bg_sprite))
        {
            s.visible = mwnd_visible && self.mwnd.waku.bg_image.is_some();
            s.alpha = anim_alpha;
            s.image_id = self.mwnd.waku.bg_image;
        }

        if let Some(sprite_id) = self.mwnd.waku.filter_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                let image_id = self
                    .mwnd
                    .waku
                    .filter_image
                    .or(self.mwnd.waku.solid_filter_image);
                let visible = mwnd_visible && image_id.is_some();
                s.visible = visible;
                s.image_id = image_id;
                const GET_FILTER_COLOR_R: i32 = 84;
                const GET_FILTER_COLOR_G: i32 = 91;
                const GET_FILTER_COLOR_B: i32 = 92;
                const GET_FILTER_COLOR_A: i32 = 93;
                let cfg = &syscom.config_int;
                let (_filter_r, _filter_g, _filter_b, filter_a) = self.mwnd.waku.filter_color;
                let has_filter_texture = self.mwnd.waku.filter_image.is_some();

                s.alpha = anim_alpha;
                s.tr = if self.mwnd.waku.filter_config_tr {
                    cfg.get(&GET_FILTER_COLOR_A)
                        .copied()
                        .unwrap_or(128)
                        .clamp(0, 255) as u8
                } else if has_filter_texture {
                    255
                } else {
                    filter_a
                };

                s.color_rate = 0;
                s.color_r = 255;
                s.color_g = 255;
                s.color_b = 255;
                s.mask_mode = 0;
                if self.mwnd.waku.filter_config_color {
                    s.color_add_r = cfg
                        .get(&GET_FILTER_COLOR_R)
                        .copied()
                        .unwrap_or(0)
                        .clamp(0, 255) as u8;
                    s.color_add_g = cfg
                        .get(&GET_FILTER_COLOR_G)
                        .copied()
                        .unwrap_or(0)
                        .clamp(0, 255) as u8;
                    s.color_add_b = cfg
                        .get(&GET_FILTER_COLOR_B)
                        .copied()
                        .unwrap_or(0)
                        .clamp(0, 255) as u8;
                } else {
                    s.color_add_r = 0;
                    s.color_add_g = 0;
                    s.color_add_b = 0;
                }
            }
        }

        if let Some(sprite_id) = self.mwnd.face.sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = mwnd_visible && self.mwnd.face.image.is_some();
                s.image_id = self.mwnd.face.image;
                s.alpha = anim_alpha;
            }
        }

        if let Some(sprite_id) = self.mwnd.msg.text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = mwnd_visible && self.mwnd.msg.text_image.is_some();
                s.image_id = self.mwnd.msg.text_image;
                s.alpha = anim_alpha;
            }
        }

        if let Some(sprite_id) = self.mwnd.name.text_sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = mwnd_visible && self.mwnd.name.text_image.is_some();
                s.image_id = self.mwnd.name.text_image;
                s.alpha = anim_alpha;
            }
        }

        if let Some(sprite_id) = self.mwnd.key_icon.sprite {
            if let Some(s) = layers
                .layer_mut(ui_layer)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                s.visible = mwnd_visible
                    && self.mwnd.key_icon.appear
                    && self.mwnd.key_icon.image.is_some();
                s.image_id = self.mwnd.key_icon.image;
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
        self.mwnd.key_icon.appear = false;
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
            self.mwnd.waku.filter_size = None;
            self.mwnd.waku.solid_filter_image = None;
        }
        self.mwnd.waku.filter_margin = proj.filter_margin.unwrap_or((0, 0, 0, 0));
        let next_filter_color = proj.filter_color.unwrap_or((0, 0, 255, 128));
        if self.mwnd.waku.filter_color != next_filter_color {
            self.mwnd.waku.solid_filter_image = None;
        }
        self.mwnd.waku.filter_color = next_filter_color;
        self.mwnd.waku.filter_config_color = proj.filter_config_color;
        self.mwnd.waku.filter_config_tr = proj.filter_config_tr;

        if self.mwnd.key_icon.key_file != proj.key_icon_file
            || self.mwnd.key_icon.page_file != proj.page_icon_file
        {
            self.mwnd.key_icon.image = None;
            self.mwnd.key_icon.file = None;
            self.mwnd.key_icon.size = None;
            self.mwnd.key_icon.anime_start = None;
        }
        self.mwnd.key_icon.key_file = proj.key_icon_file.clone();
        self.mwnd.key_icon.key_pat_cnt = proj.key_icon_pat_cnt.max(1);
        self.mwnd.key_icon.key_speed = proj.key_icon_speed.max(1);
        self.mwnd.key_icon.page_file = proj.page_icon_file.clone();
        self.mwnd.key_icon.page_pat_cnt = proj.page_icon_pat_cnt.max(1);
        self.mwnd.key_icon.page_speed = proj.page_icon_speed.max(1);
        self.mwnd.key_icon.appear = proj.key_icon_appear;
        if self.mwnd.key_icon.mode != proj.key_icon_mode {
            self.mwnd.key_icon.mode = proj.key_icon_mode;
            self.mwnd.key_icon.anime_start = None;
            self.mwnd.key_icon.image = None;
        }
        self.mwnd.key_icon.icon_pos_type = proj.icon_pos_type;
        self.mwnd.key_icon.icon_pos_base = proj.icon_pos_base;
        self.mwnd.key_icon.icon_pos = if proj.icon_pos_type == 1 {
            proj.key_icon_pos
                .map(|(x, y)| (x, y, 0))
                .or(proj.icon_pos)
                .unwrap_or((0, 0, 0))
        } else {
            proj.icon_pos.unwrap_or((0, 0, 0))
        };

        self.set_mwnd_window_state(
            proj.window_pos,
            proj.window_size,
            proj.message_pos,
            proj.message_margin,
            proj.window_moji_cnt,
            proj.moji_size,
            proj.moji_space,
            proj.mwnd_extend_type,
            proj.moji_color,
            proj.shadow_color,
            proj.fuchi_color,
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
        message_pos: Option<(i64, i64)>,
        message_margin: Option<(i64, i64, i64, i64)>,
        window_moji_cnt: Option<(i64, i64)>,
        moji_size: Option<i64>,
        moji_space: Option<(i64, i64)>,
        mwnd_extend_type: i64,
        moji_color: Option<i64>,
        shadow_color: Option<i64>,
        fuchi_color: Option<i64>,
        face_file: Option<&str>,
        face_no: i64,
        rep_pos: Option<(i64, i64)>,
        slide_enabled: bool,
        slide_time: i64,
    ) {
        self.mwnd.window.pos = window_pos.map(|(x, y)| (x as i32, y as i32));
        self.mwnd.window.size = window_size.map(|(w, h)| (w.max(1) as u32, h.max(1) as u32));
        self.mwnd.window.message_pos = message_pos.map(|(x, y)| (x as i32, y as i32));
        self.mwnd.window.message_margin = message_margin;
        self.mwnd.window.moji_cnt = window_moji_cnt;
        self.mwnd.window.moji_size = moji_size;
        self.mwnd.window.moji_space = moji_space;
        self.mwnd.window.extend_type = mwnd_extend_type;
        self.mwnd.window.moji_color = moji_color;
        self.mwnd.window.shadow_color = shadow_color;
        self.mwnd.window.fuchi_color = fuchi_color;
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
        self.mwnd.window.message_pos = None;
        self.mwnd.window.message_margin = None;
        self.mwnd.window.moji_cnt = None;
        self.mwnd.window.moji_size = None;
        self.mwnd.window.moji_space = None;
        self.mwnd.window.extend_type = 0;
        self.mwnd.window.moji_color = None;
        self.mwnd.window.shadow_color = None;
        self.mwnd.window.fuchi_color = None;
        self.mwnd.waku.bg_file = None;
        self.mwnd.waku.filter_file = None;
        self.mwnd.projection_active = false;
        self.mwnd.waku.bg_image = None;
        self.mwnd.waku.filter_image = None;
        self.mwnd.waku.solid_filter_image = None;
        self.mwnd.waku.bg_size = None;
        self.mwnd.waku.filter_size = None;
        self.mwnd.waku.filter_margin = (0, 0, 0, 0);
        self.mwnd.waku.filter_color = (0, 0, 255, 128);
        self.mwnd.waku.filter_config_color = true;
        self.mwnd.waku.filter_config_tr = true;
        self.mwnd.key_icon = MwndKeyIconRuntime::default();
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
        self.mwnd.key_icon.appear = false;
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
        self.begin_wait_message_with_icon_mode(0);
    }

    pub fn begin_wait_page_message(&mut self) {
        self.begin_wait_message_with_icon_mode(1);
    }

    fn begin_wait_message_with_icon_mode(&mut self, icon_mode: i64) {
        self.mwnd.msg.waiting = true;
        self.mwnd.msg.wait_started_at = Some(Instant::now());
        self.mwnd.msg.wait_message_len =
            self.mwnd.msg.text.as_deref().unwrap_or("").chars().count();
        self.mwnd.key_icon.appear = true;
        if self.mwnd.key_icon.mode != icon_mode {
            self.mwnd.key_icon.mode = icon_mode;
            self.mwnd.key_icon.anime_start = None;
            self.mwnd.key_icon.image = None;
        }
    }

    pub fn reveal_message_now(&mut self) {
        let total = self.mwnd.msg.text.as_deref().unwrap_or("").chars().count();
        if self.mwnd.msg.visible_chars != total {
            self.mwnd.msg.visible_chars = total;
            self.mwnd.msg.reveal_base = total;
            self.mwnd.msg.reveal_start = None;
            self.mwnd.msg.text_dirty = true;
        }
    }

    pub fn message_wait_text_fully_revealed(&self) -> bool {
        self.message_fully_revealed()
    }

    pub fn message_waiting(&self) -> bool {
        self.mwnd.msg.waiting
    }

    pub fn message_visible_chars(&self) -> usize {
        self.mwnd.msg.visible_chars
    }

    pub fn message_wait_message_len(&self) -> usize {
        self.mwnd.msg.wait_message_len
    }

    pub fn needs_continuous_frame(
        &self,
        script: &ScriptRuntimeState,
        syscom: &SyscomRuntimeState,
    ) -> bool {
        if self.mwnd.anim.started_at.is_some() {
            return true;
        }
        if self.mwnd.msg.slide_started_at.is_some() {
            return true;
        }
        if self.mwnd.msg.reveal_start.is_some() && message_speed_ms(script, syscom).is_some() {
            return true;
        }
        if self.mwnd.msg.waiting && !self.message_fully_revealed() {
            return true;
        }
        if self.mwnd.key_icon.appear {
            let pat_cnt = if self.mwnd.key_icon.mode == 1 {
                self.mwnd.key_icon.page_pat_cnt
            } else {
                self.mwnd.key_icon.key_pat_cnt
            };
            return pat_cnt > 1;
        }
        false
    }

    pub fn end_wait_message(&mut self) -> bool {
        self.mwnd.msg.waiting = false;
        self.mwnd.msg.wait_started_at = None;
        self.mwnd.key_icon.appear = false;

        if self.mwnd.msg.clear_on_wait_end {
            self.mwnd.msg.clear_on_wait_end = false;
            self.clear_message();
            true
        } else {
            false
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
                    } else if let Ok(id) = images.load_g00(raw, 0) {
                        self.mwnd.waku.filter_image = Some(id);
                    } else if let Ok(id) = images.load_bg(raw) {
                        self.mwnd.waku.filter_image = Some(id);
                    }
                    if let Some(id) = self.mwnd.waku.filter_image {
                        if let Some(img) = images.get(id) {
                            self.mwnd.waku.filter_size = Some((img.width, img.height));
                        }
                    }
                }
            }
        }
        if self.mwnd.waku.filter_image.is_none() && self.mwnd.waku.solid_filter_image.is_none() {
            let (r, g, b, _a) = self.mwnd.waku.filter_color;
            self.mwnd.waku.solid_filter_image = Some(images.solid_rgba((r, g, b, 255)));
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

    fn refresh_key_icon_image(
        &mut self,
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
    ) {
        let (file, pat_cnt, speed) = if self.mwnd.key_icon.mode == 1 {
            (
                self.mwnd.key_icon.page_file.clone(),
                self.mwnd.key_icon.page_pat_cnt,
                self.mwnd.key_icon.page_speed,
            )
        } else {
            (
                self.mwnd.key_icon.key_file.clone(),
                self.mwnd.key_icon.key_pat_cnt,
                self.mwnd.key_icon.key_speed,
            )
        };
        let Some(raw) = file.filter(|s| !s.is_empty()) else {
            self.mwnd.key_icon.image = None;
            self.mwnd.key_icon.file = None;
            self.mwnd.key_icon.size = None;
            return;
        };

        if self.mwnd.key_icon.anime_start.is_none() {
            self.mwnd.key_icon.anime_start = Some(Instant::now());
        }
        let elapsed_ms = self
            .mwnd
            .key_icon
            .anime_start
            .map(|t| t.elapsed().as_millis() as i64)
            .unwrap_or(0);
        let pat_cnt = pat_cnt.max(1);
        let speed = speed.max(1);
        let pat = (elapsed_ms / speed) % pat_cnt;
        if self.mwnd.key_icon.file.as_deref() == Some(raw.as_str())
            && self.mwnd.key_icon.cached_mode == self.mwnd.key_icon.mode
            && self.mwnd.key_icon.cached_pat == pat
            && self.mwnd.key_icon.image.is_some()
        {
            return;
        }

        let mut loaded = None;
        if let Ok(id) = images.load_g00(&raw, pat.max(0) as u32) {
            loaded = Some(id);
        } else if let Ok(id) = images.load_bg_frame(&raw, pat.max(0) as usize) {
            loaded = Some(id);
        } else {
            let path = project_dir.join(&raw);
            if path.exists() {
                if let Ok(id) = images.load_file(&path, pat.max(0) as usize) {
                    loaded = Some(id);
                }
            }
        }

        self.mwnd.key_icon.image = loaded;
        self.mwnd.key_icon.file = Some(raw);
        self.mwnd.key_icon.cached_mode = self.mwnd.key_icon.mode;
        self.mwnd.key_icon.cached_pat = pat;
        self.mwnd.key_icon.size =
            loaded.and_then(|id| images.get(id).map(|img| (img.width, img.height)));
    }

    fn refresh_text_images(
        &mut self,
        images: &mut crate::image_manager::ImageManager,
        w: u32,
        h: u32,
        script: &ScriptRuntimeState,
    ) {
        let msg_style = self.mwnd_message_text_style(script);
        let name_style = self.mwnd_name_text_style(script);
        if self.mwnd.msg.text_dirty {
            let (x, y, mw, mh) = self.msg_rect(w, h);
            let _ = (x, y);
            let font_size = self.message_font_px() as f32;
            self.mwnd.msg.text_image = self.font_cache.render_mwnd_text_styled(
                images,
                &self.visible_message_text(),
                font_size,
                mw,
                mh,
                self.mwnd.window.moji_space,
                msg_style,
            );
            self.mwnd.msg.text_dirty = false;
        }

        if self.mwnd.name.text_dirty {
            let (x, y, mw, mh) = self.name_rect(w, h);
            let _ = (x, y);
            let font_size = self.name_font_px() as f32;
            self.mwnd.name.text_image = self.font_cache.render_mwnd_text_styled(
                images,
                self.mwnd.name.text.as_deref().unwrap_or(""),
                font_size,
                mw,
                mh,
                self.mwnd.window.moji_space,
                name_style,
            );
            self.mwnd.name.text_dirty = false;
        }
    }

    fn sync_editbox_overlay(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        images: &mut crate::image_manager::ImageManager,
        editbox_lists: &HashMap<u32, EditBoxListState>,
        focused_editbox: Option<(u32, usize)>,
    ) {
        let ui_layer = Self::ensure_layer(layers, &mut self.editbox.layer);
        if self.editbox.bg_image.is_none() {
            self.editbox.bg_image = Some(images.solid_rgba((255, 255, 255, 230)));
        }
        if self.editbox.focused_bg_image.is_none() {
            self.editbox.focused_bg_image = Some(images.solid_rgba((255, 255, 220, 245)));
        }

        let normal_bg_image = self.editbox.bg_image;
        let focused_bg_image = self.editbox.focused_bg_image;
        let mut active_keys: Vec<(u32, usize)> = Vec::new();
        for (form_id, list) in editbox_lists.iter() {
            for (idx, eb) in list.boxes.iter().enumerate() {
                let key = (*form_id, idx);
                if !eb.created || !eb.visible || eb.window_w <= 0 || eb.window_h <= 0 {
                    continue;
                }
                active_keys.push(key);
                let focused = focused_editbox == Some(key);
                let entry = self.editbox.entries.entry(key).or_default();
                let bg_sprite = Self::ensure_text_sprite(layers, ui_layer, &mut entry.bg_sprite);
                let text_sprite =
                    Self::ensure_text_sprite(layers, ui_layer, &mut entry.text_sprite);
                let w = eb.window_w.max(1) as u32;
                let h = eb.window_h.max(1) as u32;
                let font_px = eb.window_moji_size.max(12) as u32;
                let display_text = editbox_display_text(&eb.text, eb.cursor_pos, focused);

                if entry.text_image.is_none()
                    || entry.last_text != display_text
                    || entry.last_w != w
                    || entry.last_h != h
                    || entry.last_font_px != font_px
                    || entry.last_focused != focused
                {
                    entry.text_image = self.font_cache.render_text(
                        images,
                        &display_text,
                        font_px as f32,
                        w.saturating_sub(8).max(1),
                        h.max(1),
                    );
                    entry.last_text = display_text;
                    entry.last_w = w;
                    entry.last_h = h;
                    entry.last_font_px = font_px;
                    entry.last_focused = focused;
                }

                if let Some(s) = layers
                    .layer_mut(ui_layer)
                    .and_then(|l| l.sprite_mut(bg_sprite))
                {
                    s.visible = true;
                    s.image_id = if focused {
                        focused_bg_image
                    } else {
                        normal_bg_image
                    };
                    s.fit = SpriteFit::PixelRect;
                    s.size_mode = SpriteSizeMode::Explicit {
                        width: w,
                        height: h,
                    };
                    s.x = eb.window_x;
                    s.y = eb.window_y;
                    s.order = 1_950_000 + idx as i32 * 2;
                    s.alpha = 255;
                }
                if let Some(s) = layers
                    .layer_mut(ui_layer)
                    .and_then(|l| l.sprite_mut(text_sprite))
                {
                    s.visible = entry.text_image.is_some();
                    s.image_id = entry.text_image;
                    s.fit = SpriteFit::PixelRect;
                    if entry.text_image.is_some() {
                        s.size_mode = SpriteSizeMode::Intrinsic;
                    } else {
                        s.size_mode = SpriteSizeMode::Explicit {
                            width: w.saturating_sub(8).max(1),
                            height: h,
                        };
                    }
                    s.x = eb.window_x.saturating_add(4);
                    s.y = eb.window_y;
                    s.order = 1_950_001 + idx as i32 * 2;
                    s.alpha = 255;
                }
            }
        }

        for (key, entry) in self.editbox.entries.iter_mut() {
            if active_keys.iter().any(|x| x == key) {
                continue;
            }
            if let Some(sprite_id) = entry.bg_sprite {
                if let Some(s) = layers
                    .layer_mut(ui_layer)
                    .and_then(|l| l.sprite_mut(sprite_id))
                {
                    s.visible = false;
                }
            }
            if let Some(sprite_id) = entry.text_sprite {
                if let Some(s) = layers
                    .layer_mut(ui_layer)
                    .and_then(|l| l.sprite_mut(sprite_id))
                {
                    s.visible = false;
                }
            }
        }
    }

    pub fn set_msg_back_projection(&mut self, projection: Option<MsgBackUiProjection>) {
        match projection {
            Some(next) => {
                self.msg_back.projection = Some(next);
                self.msg_back.text_dirty = true;
            }
            None => {
                if self.msg_back.projection.is_some() {
                    self.msg_back.projection = None;
                    self.msg_back.text_dirty = true;
                }
            }
        }
    }

    pub fn msg_back_slider_size(&self) -> Option<(u32, u32)> {
        self.msg_back.slider.size
    }

    pub fn msg_back_slider_screen_pos(&self) -> Option<(i32, i32)> {
        let projection = self.msg_back.projection.as_ref()?;
        Some((
            projection.window_x + projection.slider_pos.0,
            projection.window_y + projection.slider_pos.1,
        ))
    }

    pub fn msg_back_hit_action(&self, x: i32, y: i32) -> Option<MsgBackHitAction> {
        let projection = self.msg_back.projection.as_ref()?;
        if let Some(action) = Self::msg_back_button_hit(
            projection,
            &self.msg_back.slider,
            projection.slider_pos,
            x,
            y,
            MsgBackHitAction::Slider,
        ) {
            return Some(action);
        }
        if let Some(action) = Self::msg_back_button_hit(
            projection,
            &self.msg_back.close_btn,
            projection.close_btn_pos,
            x,
            y,
            MsgBackHitAction::Close,
        ) {
            return Some(action);
        }
        if let Some(action) = Self::msg_back_button_hit(
            projection,
            &self.msg_back.msg_up_btn,
            projection.msg_up_btn_pos,
            x,
            y,
            MsgBackHitAction::Up,
        ) {
            return Some(action);
        }
        if let Some(action) = Self::msg_back_button_hit(
            projection,
            &self.msg_back.msg_down_btn,
            projection.msg_down_btn_pos,
            x,
            y,
            MsgBackHitAction::Down,
        ) {
            return Some(action);
        }
        None
    }

    fn msg_back_button_hit(
        projection: &MsgBackUiProjection,
        button: &MsgBackButtonRuntime,
        pos: (i32, i32),
        x: i32,
        y: i32,
        action: MsgBackHitAction,
    ) -> Option<MsgBackHitAction> {
        let (w, h) = button.size?;
        if w == 0 || h == 0 || button.image.is_none() {
            return None;
        }
        let (center_x, center_y) = button.center.unwrap_or((0, 0));
        let left = projection.window_x + pos.0 - center_x;
        let top = projection.window_y + pos.1 - center_y;
        let right = left.saturating_add(w as i32);
        let bottom = top.saturating_add(h as i32);
        (left <= x && x < right && top <= y && y < bottom).then_some(action)
    }

    fn hide_msg_back_sprites(&mut self, layers: &mut crate::layer::LayerManager) {
        let Some(ui_layer) = self.mwnd.layer else {
            return;
        };
        let mut hide = |slot: Option<SpriteId>| {
            if let Some(sprite_id) = slot {
                if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(sprite_id)) {
                    s.visible = false;
                }
            }
        };
        hide(self.msg_back.waku_sprite);
        hide(self.msg_back.filter_sprite);
        hide(self.msg_back.text_sprite);
        for entry in &self.msg_back.text_entries {
            hide(entry.sprite);
        }
        for sep in &self.msg_back.separators {
            hide(sep.sprite);
        }
        for button in &self.msg_back.koe_buttons {
            hide(button.sprite);
        }
        for button in &self.msg_back.load_buttons {
            hide(button.sprite);
        }
        hide(self.msg_back.close_btn.sprite);
        hide(self.msg_back.msg_up_btn.sprite);
        hide(self.msg_back.msg_down_btn.sprite);
        hide(self.msg_back.slider.sprite);
        for button in &self.msg_back.ex_buttons {
            hide(button.sprite);
        }
    }

    fn ensure_msg_back_button_sprite(
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
        button: &mut MsgBackButtonRuntime,
    ) -> SpriteId {
        if let Some(id) = button.sprite {
            if layers.layer(ui_layer).and_then(|l| l.sprite(id)).is_some() {
                return id;
            }
        }
        let sprite_id = layers
            .layer_mut(ui_layer)
            .expect("ui_layer exists")
            .create_sprite();
        button.sprite = Some(sprite_id);
        sprite_id
    }

    fn load_msg_back_image(
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
        file: Option<&String>,
    ) -> Option<ImageId> {
        let raw = file.map(|s| s.trim()).filter(|s| !s.is_empty())?;
        if let Ok(id) = images.load_g00(raw, 0) {
            return Some(id);
        }
        if let Ok(id) = images.load_bg_frame(raw, 0) {
            return Some(id);
        }
        let path = project_dir.join(raw);
        if path.exists() {
            if let Ok(id) = images.load_file(&path, 0) {
                return Some(id);
            }
        }
        None
    }

    fn refresh_msg_back_button_image(
        button: &mut MsgBackButtonRuntime,
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
        file: Option<&String>,
    ) {
        if button.cached_file.as_ref() == file {
            return;
        }
        button.image = Self::load_msg_back_image(images, project_dir, file);
        button.size = button
            .image
            .and_then(|id| images.get(id).map(|img| (img.width, img.height)));
        button.center = button
            .image
            .and_then(|id| images.get(id).map(|img| (img.center_x, img.center_y)));
        button.cached_file = file.cloned();
    }

    fn apply_msg_back_pct_anchor(
        sprite: &mut Sprite,
        images: &crate::image_manager::ImageManager,
        image: Option<ImageId>,
    ) {
        if let Some(img) = image.and_then(|id| images.get(id)) {
            sprite.object_anchor = true;
            sprite.texture_center_x = img.center_x as f32;
            sprite.texture_center_y = img.center_y as f32;
        } else {
            sprite.object_anchor = false;
            sprite.texture_center_x = 0.0;
            sprite.texture_center_y = 0.0;
        }
    }

    fn sync_msg_back_button_sprite(
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
        images: &crate::image_manager::ImageManager,
        button: &mut MsgBackButtonRuntime,
        projection: &MsgBackUiProjection,
        pos: (i32, i32),
        order: i32,
    ) {
        let sprite_id = Self::ensure_msg_back_button_sprite(layers, ui_layer, button);
        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(sprite_id)) {
            s.visible = button.image.is_some();
            s.image_id = button.image;
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Intrinsic;
            s.x = projection.window_x + pos.0;
            s.y = projection.window_y + pos.1;
            s.order = order;
            s.alpha = 255;
            s.tr = 255;
            s.alpha_test = true;
            s.alpha_blend = true;
            s.color_rate = 0;
            s.color_add_r = 0;
            s.color_add_g = 0;
            s.color_add_b = 0;
            s.color_r = 0;
            s.color_g = 0;
            s.color_b = 0;
            s.mask_mode = 0;
            Self::apply_msg_back_pct_anchor(s, images, button.image);
            s.dst_clip = None;
            s.src_clip = None;
        }
    }

    fn hide_msg_back_button_sprite(
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
        button: &MsgBackButtonRuntime,
    ) {
        if let Some(sprite_id) = button.sprite {
            if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(sprite_id)) {
                s.visible = false;
            }
        }
    }

    fn sync_msg_back_abs_button_sprite(
        layers: &mut crate::layer::LayerManager,
        ui_layer: LayerId,
        images: &crate::image_manager::ImageManager,
        button: &mut MsgBackButtonRuntime,
        pos: (i32, i32),
        order: i32,
        clip: Option<crate::layer::ClipRect>,
    ) {
        let sprite_id = Self::ensure_msg_back_button_sprite(layers, ui_layer, button);
        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(sprite_id)) {
            s.visible = button.image.is_some();
            s.image_id = button.image;
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Intrinsic;
            s.x = pos.0;
            s.y = pos.1;
            s.order = order;
            s.alpha = 255;
            s.tr = 255;
            s.alpha_test = true;
            s.alpha_blend = true;
            s.color_rate = 0;
            s.color_add_r = 0;
            s.color_add_g = 0;
            s.color_add_b = 0;
            s.color_r = 0;
            s.color_g = 0;
            s.color_b = 0;
            s.mask_mode = 0;
            Self::apply_msg_back_pct_anchor(s, images, button.image);
            s.dst_clip = clip;
            s.src_clip = None;
        }
    }

    fn sync_msg_back_ui(
        &mut self,
        layers: &mut crate::layer::LayerManager,
        images: &mut crate::image_manager::ImageManager,
        project_dir: &Path,
    ) {
        let Some(projection) = self.msg_back.projection.clone() else {
            self.hide_msg_back_sprites(layers);
            return;
        };
        let ui_layer = Self::ensure_layer(layers, &mut self.mwnd.layer);

        let waku_sprite = Self::ensure_text_sprite(layers, ui_layer, &mut self.msg_back.waku_sprite);
        let filter_sprite = Self::ensure_text_sprite(layers, ui_layer, &mut self.msg_back.filter_sprite);
        let old_text_sprite = Self::ensure_text_sprite(layers, ui_layer, &mut self.msg_back.text_sprite);
        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(old_text_sprite)) {
            s.visible = false;
        }

        if self.msg_back.cached_waku_file.as_ref() != projection.waku_file.as_ref() {
            self.msg_back.waku_image = Self::load_msg_back_image(images, project_dir, projection.waku_file.as_ref());
            self.msg_back.cached_waku_file = projection.waku_file.clone();
        }
        if self.msg_back.cached_filter_file.as_ref() != projection.filter_file.as_ref() {
            self.msg_back.filter_image = Self::load_msg_back_image(images, project_dir, projection.filter_file.as_ref());
            self.msg_back.cached_filter_file = projection.filter_file.clone();
        }
        if self.msg_back.solid_filter_color != Some(projection.filter_rgba) {
            self.msg_back.solid_filter_image = Some(images.solid_rgba(projection.filter_rgba));
            self.msg_back.solid_filter_color = Some(projection.filter_rgba);
        }

        Self::refresh_msg_back_button_image(
            &mut self.msg_back.close_btn,
            images,
            project_dir,
            projection.close_btn_file.as_ref(),
        );
        Self::refresh_msg_back_button_image(
            &mut self.msg_back.msg_up_btn,
            images,
            project_dir,
            projection.msg_up_btn_file.as_ref(),
        );
        Self::refresh_msg_back_button_image(
            &mut self.msg_back.msg_down_btn,
            images,
            project_dir,
            projection.msg_down_btn_file.as_ref(),
        );
        Self::refresh_msg_back_button_image(
            &mut self.msg_back.slider,
            images,
            project_dir,
            projection.slider_file.as_ref(),
        );
        if self.msg_back.ex_buttons.len() < 4 {
            self.msg_back.ex_buttons.resize_with(4, MsgBackButtonRuntime::default);
        }
        for i in 0..4 {
            Self::refresh_msg_back_button_image(
                &mut self.msg_back.ex_buttons[i],
                images,
                project_dir,
                projection.ex_btn_files[i].as_ref(),
            );
        }

        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(waku_sprite)) {
            s.visible = self.msg_back.waku_image.is_some();
            s.image_id = self.msg_back.waku_image;
            s.fit = SpriteFit::PixelRect;
            s.size_mode = if self.msg_back.waku_image.is_some() {
                SpriteSizeMode::Intrinsic
            } else {
                SpriteSizeMode::Explicit {
                    width: projection.window_w,
                    height: projection.window_h,
                }
            };
            s.x = projection.window_x;
            s.y = projection.window_y;
            s.order = msg_back_packed_sorter_key(projection.order, projection.waku_layer_rep);
            s.alpha = 255;
            s.tr = 255;
            s.alpha_test = true;
            s.alpha_blend = true;
            s.color_rate = 0;
            s.color_add_r = 0;
            s.color_add_g = 0;
            s.color_add_b = 0;
            s.color_r = 0;
            s.color_g = 0;
            s.color_b = 0;
            s.mask_mode = 0;
            Self::apply_msg_back_pct_anchor(s, images, self.msg_back.waku_image);
            s.dst_clip = None;
            s.src_clip = None;
        }

        let filter_image = self.msg_back.filter_image.or(self.msg_back.solid_filter_image);
        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(filter_sprite)) {
            let (ml, mt, mr, mb) = projection.filter_margin;
            s.visible = filter_image.is_some();
            s.image_id = filter_image;
            s.fit = SpriteFit::PixelRect;
            if self.msg_back.filter_image.is_some() {
                s.size_mode = SpriteSizeMode::Intrinsic;
                s.x = projection.window_x;
                s.y = projection.window_y;
            } else {
                s.size_mode = SpriteSizeMode::Explicit {
                    width: (projection.window_w as i64 - ml - mr).max(1) as u32,
                    height: (projection.window_h as i64 - mt - mb).max(1) as u32,
                };
                s.x = projection.window_x + ml as i32;
                s.y = projection.window_y + mt as i32;
            }
            s.order = msg_back_packed_sorter_key(projection.order, projection.filter_layer_rep);
            let (cfg_r, cfg_g, cfg_b, cfg_a) = projection.filter_config_rgba;
            s.alpha = 255;
            s.tr = cfg_a;
            s.alpha_test = true;
            s.alpha_blend = true;
            s.color_rate = 0;
            s.color_add_r = cfg_r;
            s.color_add_g = cfg_g;
            s.color_add_b = cfg_b;
            s.color_r = 0;
            s.color_g = 0;
            s.color_b = 0;
            s.mask_mode = 0;
            if self.msg_back.filter_image.is_some() {
                Self::apply_msg_back_pct_anchor(s, images, self.msg_back.filter_image);
            } else {
                s.object_anchor = false;
                s.texture_center_x = 0.0;
                s.texture_center_y = 0.0;
            }
            s.dst_clip = None;
            s.src_clip = None;
        }

        let (dl, dt, dr, db) = projection.disp_margin;
        let clip = crate::layer::ClipRect {
            left: projection.window_x + dl as i32,
            top: projection.window_y + dt as i32,
            right: projection.window_x + projection.window_w as i32 - dr as i32,
            bottom: projection.window_y + projection.window_h as i32 - db as i32,
        };

        if self.msg_back.separators.len() < projection.separators.len() {
            self.msg_back.separators.resize_with(projection.separators.len(), MsgBackButtonRuntime::default);
        }
        for i in 0..projection.separators.len() {
            let sep = &projection.separators[i];
            Self::refresh_msg_back_button_image(
                &mut self.msg_back.separators[i],
                images,
                project_dir,
                sep.file.as_ref(),
            );
            Self::sync_msg_back_abs_button_sprite(
                layers,
                ui_layer,
                images,
                &mut self.msg_back.separators[i],
                (projection.window_x + sep.x, projection.window_y + sep.y),
                msg_back_packed_sorter_key(projection.order, projection.waku_layer_rep),
                Some(clip),
            );
        }
        for i in projection.separators.len()..self.msg_back.separators.len() {
            Self::hide_msg_back_button_sprite(layers, ui_layer, &self.msg_back.separators[i]);
        }

        if self.msg_back.text_entries.len() < projection.text_entries.len() {
            self.msg_back.text_entries.resize_with(projection.text_entries.len(), MsgBackTextRuntime::default);
        }
        for i in 0..projection.text_entries.len() {
            let entry = &projection.text_entries[i];
            let runtime = &mut self.msg_back.text_entries[i];
            let sprite_id = Self::ensure_text_sprite(layers, ui_layer, &mut runtime.sprite);
            let render_text = entry.text.replace('\u{0007}', "\n");
            runtime.image = self.font_cache.render_mwnd_text_styled_into(
                images,
                runtime.image,
                &render_text,
                projection.moji_size.max(1) as f32,
                entry.width.max(1),
                entry.height.max(1),
                projection.moji_space,
                entry.style,
            );
            if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(sprite_id)) {
                s.visible = runtime.image.is_some();
                s.image_id = runtime.image;
                s.fit = SpriteFit::PixelRect;
                if runtime.image.is_some() {
                    s.size_mode = SpriteSizeMode::Intrinsic;
                } else {
                    s.size_mode = SpriteSizeMode::Explicit {
                        width: entry.width.max(1),
                        height: entry.height.max(1),
                    };
                }
                s.x = projection.window_x + entry.x;
                s.y = projection.window_y + entry.y;
                s.order = msg_back_packed_sorter_key(projection.order, 0);
                s.alpha = 255;
                s.tr = 255;
                s.alpha_test = false;
                s.alpha_blend = true;
                s.color_rate = 0;
                s.color_add_r = 0;
                s.color_add_g = 0;
                s.color_add_b = 0;
                s.color_r = 0;
                s.color_g = 0;
                s.color_b = 0;
                s.mask_mode = 0;
                s.src_clip = None;
                s.dst_clip = Some(clip);
            }
        }
        for i in projection.text_entries.len()..self.msg_back.text_entries.len() {
            if let Some(sprite_id) = self.msg_back.text_entries[i].sprite {
                if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(sprite_id)) {
                    s.visible = false;
                }
            }
        }

        if self.msg_back.koe_buttons.len() < projection.koe_buttons.len() {
            self.msg_back.koe_buttons.resize_with(projection.koe_buttons.len(), MsgBackButtonRuntime::default);
        }
        for i in 0..projection.koe_buttons.len() {
            let btn = &projection.koe_buttons[i];
            Self::refresh_msg_back_button_image(
                &mut self.msg_back.koe_buttons[i],
                images,
                project_dir,
                btn.file.as_ref(),
            );
            Self::sync_msg_back_abs_button_sprite(
                layers,
                ui_layer,
                images,
                &mut self.msg_back.koe_buttons[i],
                (projection.window_x + btn.x, projection.window_y + btn.y),
                msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
                Some(clip),
            );
        }
        for i in projection.koe_buttons.len()..self.msg_back.koe_buttons.len() {
            Self::hide_msg_back_button_sprite(layers, ui_layer, &self.msg_back.koe_buttons[i]);
        }

        if self.msg_back.load_buttons.len() < projection.load_buttons.len() {
            self.msg_back.load_buttons.resize_with(projection.load_buttons.len(), MsgBackButtonRuntime::default);
        }
        for i in 0..projection.load_buttons.len() {
            let btn = &projection.load_buttons[i];
            Self::refresh_msg_back_button_image(
                &mut self.msg_back.load_buttons[i],
                images,
                project_dir,
                btn.file.as_ref(),
            );
            Self::sync_msg_back_abs_button_sprite(
                layers,
                ui_layer,
                images,
                &mut self.msg_back.load_buttons[i],
                (projection.window_x + btn.x, projection.window_y + btn.y),
                msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
                Some(clip),
            );
        }
        for i in projection.load_buttons.len()..self.msg_back.load_buttons.len() {
            Self::hide_msg_back_button_sprite(layers, ui_layer, &self.msg_back.load_buttons[i]);
        }

        Self::sync_msg_back_button_sprite(
            layers,
            ui_layer,
            images,
            &mut self.msg_back.close_btn,
            &projection,
            projection.close_btn_pos,
            msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
        );
        Self::sync_msg_back_button_sprite(
            layers,
            ui_layer,
            images,
            &mut self.msg_back.msg_up_btn,
            &projection,
            projection.msg_up_btn_pos,
            msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
        );
        Self::sync_msg_back_button_sprite(
            layers,
            ui_layer,
            images,
            &mut self.msg_back.msg_down_btn,
            &projection,
            projection.msg_down_btn_pos,
            msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
        );
        Self::sync_msg_back_button_sprite(
            layers,
            ui_layer,
            images,
            &mut self.msg_back.slider,
            &projection,
            projection.slider_pos,
            msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
        );
        for i in 0..4 {
            let button = &mut self.msg_back.ex_buttons[i];
            Self::sync_msg_back_button_sprite(
                layers,
                ui_layer,
                images,
                button,
                &projection,
                projection.ex_btn_pos[i],
                msg_back_packed_sorter_key(projection.order, projection.moji_layer_rep),
            );
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
            if let Some(ui_layer) = self.mwnd.layer {
                if let Some(sprite_id) = self.sys.bg_sprite {
                    if let Some(s) = layers
                        .layer_mut(ui_layer)
                        .and_then(|l| l.sprite_mut(sprite_id))
                    {
                        s.visible = false;
                    }
                }
                if let Some(sprite_id) = self.sys.text_sprite {
                    if let Some(s) = layers
                        .layer_mut(ui_layer)
                        .and_then(|l| l.sprite_mut(sprite_id))
                    {
                        s.visible = false;
                    }
                }
            }
            return;
        }
        let ui_layer = Self::ensure_layer(layers, &mut self.mwnd.layer);
        let bg = self.ensure_sys_bg_sprite(layers, ui_layer);
        let text = Self::ensure_text_sprite(layers, ui_layer, &mut self.sys.text_sprite);

        if self.sys.bg_image.is_none() {
            self.sys.bg_image = Some(images.solid_rgba((0, 0, 0, 180)));
        }

        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(bg)) {
            s.visible = self.sys.active;
            s.image_id = self.sys.bg_image;
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
            s.visible = self.sys.active && self.sys.text_image.is_some();
            s.image_id = self.sys.text_image;
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
        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(text)) {
            s.visible = self.sys.active && self.sys.text_image.is_some();
            s.image_id = self.sys.text_image;
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

fn editbox_display_text(text: &str, cursor_pos: usize, focused: bool) -> String {
    if !focused {
        return text.to_string();
    }
    let pos = if text.is_char_boundary(cursor_pos.min(text.len())) {
        cursor_pos.min(text.len())
    } else {
        text.char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i < cursor_pos)
            .last()
            .unwrap_or(0)
    };
    let mut out = String::with_capacity(text.len() + 1);
    out.push_str(&text[..pos]);
    out.push('|');
    out.push_str(&text[pos..]);
    out
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
    const GET_MESSAGE_SPEED: i32 = crate::runtime::constants::elm_value::SYSCOM_GET_MESSAGE_SPEED;
    const GET_MESSAGE_NOWAIT: i32 = crate::runtime::constants::elm_value::SYSCOM_GET_MESSAGE_NOWAIT;

    if script.msg_nowait || *syscom.config_int.get(&GET_MESSAGE_NOWAIT).unwrap_or(&0) != 0 {
        return None;
    }
    let speed = if script.msg_speed >= 0 {
        script.msg_speed
    } else {
        *syscom.config_int.get(&GET_MESSAGE_SPEED).unwrap_or(&20)
    };
    if speed <= 0 {
        None
    } else {
        Some(speed as u64)
    }
}

impl UiRuntime {
    fn scan_font_dir(&mut self, project_dir: &Path) {
        if self.font_scanned {
            return;
        }
        self.font_scanned = true;
        for dir in [project_dir.join("font"), project_dir.join("fonts")] {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
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
}
