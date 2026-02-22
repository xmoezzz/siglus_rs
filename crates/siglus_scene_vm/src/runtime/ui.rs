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

/// A minimal UI runtime that owns a couple of fixed layers/sprites.
#[derive(Debug, Default, Clone)]
pub struct UiRuntime {
    /// Dedicated layer for message/text UI.
    pub ui_layer: Option<LayerId>,
    /// Background sprite of the message window.
    pub msg_bg_sprite: Option<SpriteId>,
    /// Whether message window background is visible.
    pub msg_bg_visible: bool,
    /// Cached bg image id for message window background.
    pub msg_bg_image: Option<ImageId>,
    /// Current message string (debug-only for now).
    pub current_message: Option<String>,
    /// Current speaker/name string (debug-only for now).
    pub current_name: Option<String>,
    /// Whether we are in a blocking message wait.
    pub waiting_message: bool,

    /// If set, we clear the current message when a message-wait ends.
    ///
    /// This approximates page-break behavior (e.g., PP/R/PAGE in MWND).
    pub clear_message_on_wait_end: bool,
}

impl UiRuntime {
    fn ensure_layer(layers: &mut crate::layer::LayerManager, want: &mut Option<LayerId>) -> LayerId {
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

    /// Ensure fixed UI sprites exist and are laid out for the given screen size.
    pub fn sync_layout(&mut self, layers: &mut crate::layer::LayerManager, w: u32, h: u32) {
        let ui_layer = Self::ensure_layer(layers, &mut self.ui_layer);
        let bg_sprite = self.ensure_msg_bg_sprite(layers, ui_layer);

        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(bg_sprite)) {
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Explicit {
                width: w,
                height: (h / 3).max(1),
            };
            s.x = 0;
            s.y = (h as i32) - (h as i32 / 3);
            s.order = 1_000_000; // always above typical sprites
        }
    }

    /// Called once per frame to update UI and apply visibility.
    pub fn tick(&mut self, layers: &mut crate::layer::LayerManager, w: u32, h: u32) {
        self.sync_layout(layers, w, h);

        let Some(ui_layer) = self.ui_layer else { return; };
        let Some(bg_sprite) = self.msg_bg_sprite else { return; };

        if let Some(s) = layers.layer_mut(ui_layer).and_then(|l| l.sprite_mut(bg_sprite)) {
            s.visible = self.msg_bg_visible;
            if let Some(img) = self.msg_bg_image {
                s.image_id = Some(img);
            }
        }
    }

    pub fn set_message_bg(&mut self, img: ImageId) {
        self.msg_bg_image = Some(img);
    }

    pub fn show_message_bg(&mut self, on: bool) {
        self.msg_bg_visible = on;
    }

    pub fn set_message(&mut self, msg: String) {
        self.current_message = Some(msg);
    }

    pub fn append_message(&mut self, msg: &str) {
        if msg.is_empty() {
            return;
        }
        match self.current_message.as_mut() {
            Some(s) => s.push_str(msg),
            None => self.current_message = Some(msg.to_string()),
        }
    }

    pub fn append_linebreak(&mut self) {
        match self.current_message.as_mut() {
            Some(s) => s.push('\n'),
            None => self.current_message = Some("\n".to_string()),
        }
    }

    pub fn set_name(&mut self, name: String) {
        self.current_name = if name.is_empty() { None } else { Some(name) };
    }

    pub fn clear_name(&mut self) {
        self.current_name = None;
    }

    pub fn clear_message(&mut self) {
        self.current_message = None;
    }

    pub fn begin_wait_message(&mut self) {
        self.waiting_message = true;
    }

    pub fn end_wait_message(&mut self) {
        self.waiting_message = false;

        if self.clear_message_on_wait_end {
            self.clear_message_on_wait_end = false;
            self.clear_message();
        }
    }

    pub fn request_clear_message_on_wait_end(&mut self) {
        self.clear_message_on_wait_end = true;
    }
}
