use crate::image_manager::ImageId;

pub type LayerId = usize;
pub type SpriteId = usize;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SpriteFit {
    /// Stretch to the entire framebuffer.
    FullScreen,
    /// Positioned in pixel coordinates with size controlled by `size_mode`.
    PixelRect,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SpriteSizeMode {
    /// Use the intrinsic image size.
    Intrinsic,
    /// Use an explicit size.
    Explicit { width: u32, height: u32 },
}

#[derive(Debug, Clone)]
pub struct Sprite {
    pub image_id: Option<ImageId>,
    pub fit: SpriteFit,
    pub size_mode: SpriteSizeMode,
    pub visible: bool,
    pub alpha: u8,
    pub x: i32,
    pub y: i32,
    /// Render order within a layer (ascending).
    pub order: i32,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            image_id: None,
            fit: SpriteFit::PixelRect,
            size_mode: SpriteSizeMode::Intrinsic,
            visible: false,
            alpha: 255,
            x: 0,
            y: 0,
            order: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layer {
    sprites: Vec<Sprite>,
}

impl Layer {
    pub fn new() -> Self {
        Self { sprites: Vec::new() }
    }

    pub fn create_sprite(&mut self) -> SpriteId {
        let id = self.sprites.len();
        self.sprites.push(Sprite::default());
        id
    }

    pub fn sprite(&self, id: SpriteId) -> Option<&Sprite> {
        self.sprites.get(id)
    }

    pub fn sprite_mut(&mut self, id: SpriteId) -> Option<&mut Sprite> {
        self.sprites.get_mut(id)
    }

    pub fn clear_all_sprites(&mut self) {
        // Keep sprite IDs stable: do not shrink the sprite vector.
        for s in &mut self.sprites {
            s.image_id = None;
            s.visible = false;
            s.alpha = 255;
            s.x = 0;
            s.y = 0;
            s.order = 0;
            s.fit = SpriteFit::PixelRect;
            s.size_mode = SpriteSizeMode::Intrinsic;
        }
    }

    fn sprite_ids_sorted(&self) -> Vec<SpriteId> {
        let mut ids: Vec<SpriteId> = (0..self.sprites.len()).collect();
        ids.sort_by(|&a, &b| {
            let oa = self.sprites[a].order;
            let ob = self.sprites[b].order;
            oa.cmp(&ob).then(a.cmp(&b))
        });
        ids
    }
}

#[derive(Debug, Clone)]
pub struct RenderSprite {
    pub layer_id: Option<LayerId>,
    pub sprite_id: Option<SpriteId>,
    pub sprite: Sprite,
}

#[derive(Debug, Default)]
pub struct LayerManager {
    bg: Sprite,
    layers: Vec<Layer>,
}

impl LayerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bg_mut(&mut self) -> &mut Sprite {
        &mut self.bg
    }

    pub fn set_bg_image(&mut self, image_id: ImageId) {
        self.bg.image_id = Some(image_id);
        self.bg.fit = SpriteFit::FullScreen;
        self.bg.size_mode = SpriteSizeMode::Intrinsic;
        self.bg.visible = true;
        self.bg.order = i32::MIN;
    }

    pub fn clear_bg(&mut self) {
        self.bg.image_id = None;
        self.bg.visible = false;
    }

    pub fn create_layer(&mut self) -> LayerId {
        let id = self.layers.len();
        self.layers.push(Layer::new());
        id
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.get_mut(id)
    }

    pub fn clear_layer(&mut self, id: LayerId) {
        if let Some(layer) = self.layers.get_mut(id) {
            layer.clear_all_sprites();
        }
    }

    pub fn clear_all(&mut self) {
        self.clear_bg();
        for layer in &mut self.layers {
            layer.clear_all_sprites();
        }
    }

    pub fn render_list(&self) -> Vec<RenderSprite> {
        let mut out = Vec::new();

        if self.bg.visible {
            if let Some(img) = self.bg.image_id {
                let mut bg = self.bg.clone();
                bg.image_id = Some(img);
                out.push(RenderSprite {
                    layer_id: None,
                    sprite_id: None,
                    sprite: bg,
                });
            }
        }

        for (layer_id, layer) in self.layers.iter().enumerate() {
            for sprite_id in layer.sprite_ids_sorted() {
                let s = &layer.sprites[sprite_id];
                if !s.visible {
                    continue;
                }
                if s.image_id.is_none() {
                    continue;
                }
                out.push(RenderSprite {
                    layer_id: Some(layer_id),
                    sprite_id: Some(sprite_id),
                    sprite: s.clone(),
                });
            }
        }

        out
    }
}
