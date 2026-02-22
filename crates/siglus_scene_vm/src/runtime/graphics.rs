//! Graphics runtime: bridges VM stage/object operations to `LayerManager` + `ImageManager`.
//!
//! This is deliberately minimal and forgiving: unknown constants should not block
//! bring-up. We map a subset of Stage/Object operations onto sprites so BG/CHR
//! changes are visible while remaining operations can be added incrementally.

use anyhow::{bail, Context, Result};

use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{LayerId, LayerManager, SpriteFit, SpriteId, SpriteSizeMode};

#[derive(Debug, Clone)]
struct ObjectState {
    is_bg: bool,

    // Render binding for non-BG objects.
    layer_id: Option<LayerId>,
    sprite_id: Option<SpriteId>,

    // Logical properties.
    file: Option<String>,
    patno: i64,
    disp: bool,
    x: i64,
    y: i64,
    layer_no: i64,
    order: i64,
    alpha: i64,
    /// Stored but not used for sorting (Siglus draw order follows tree traversal).
    z: i64,
}

impl Default for ObjectState {
    fn default() -> Self {
        Self {
            is_bg: false,
            layer_id: None,
            sprite_id: None,
            file: None,
            patno: 0,
            disp: false,
            x: 0,
            y: 0,
            layer_no: 0,
            order: 0,
            alpha: 255,
            z: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct StageState {
    layer_id: Option<LayerId>,
    objects: Vec<ObjectState>,
}

impl Default for StageState {
    fn default() -> Self {
        Self {
            layer_id: None,
            objects: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct GfxRuntime {
    /// Current logical layer number selected by named commands (LAYER/LAYER_SET).
    /// Used as a default for CHR/object operations when scripts omit an explicit layer.
    pub current_layer: i32,
    stages: [StageState; 3],
}

impl Default for GfxRuntime {
    fn default() -> Self {
        Self {
            current_layer: 0,
            stages: [StageState::default(), StageState::default(), StageState::default()],
        }
    }
}

impl GfxRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_stage(&mut self, stage: usize) -> &mut StageState {
        &mut self.stages[stage]
    }

    fn ensure_stage_layer(&mut self, layers: &mut LayerManager, stage: usize) -> LayerId {
        let st = self.ensure_stage(stage);
        if let Some(id) = st.layer_id {
            return id;
        }
        let id = layers.create_layer();
        st.layer_id = Some(id);
        id
    }

    fn ensure_object_mut(&mut self, stage: usize, obj_idx: usize) -> &mut ObjectState {
        let st = self.ensure_stage(stage);
        if st.objects.len() <= obj_idx {
            st.objects.resize_with(obj_idx + 1, Default::default);
        }
        &mut st.objects[obj_idx]
    }

    fn object(&self, stage: usize, obj_idx: usize) -> Option<&ObjectState> {
        self.stages.get(stage)?.objects.get(obj_idx)
    }

    fn load_any_image(images: &mut ImageManager, file: &str, patno: i64) -> Result<ImageId> {
        // Engine preference: g00 first, then bg fallback.
        let pat_u32 = if patno < 0 { 0 } else { patno as u32 };
        match images.load_g00(file, pat_u32) {
            Ok(id) => Ok(id),
            Err(_) => images
                .load_bg(file)
                .with_context(|| format!("failed to load image as g00/bg: {file}")),
        }
    }

    fn ensure_bound_sprite(
        &mut self,
        layers: &mut LayerManager,
        stage: usize,
        obj_idx: usize,
    ) -> Result<(LayerId, SpriteId)> {
        let st_layer = self.ensure_stage_layer(layers, stage);
        let obj = self.ensure_object_mut(stage, obj_idx);

        if obj.is_bg {
            bail!("BG object does not have a bound sprite");
        }

        if let (Some(lid), Some(sid)) = (obj.layer_id, obj.sprite_id) {
            return Ok((lid, sid));
        }

        let sid = {
            let layer = layers.layer_mut(st_layer).context("stage layer not found")?;
            layer.create_sprite()
        };

        // Initialize with sane defaults.
        if let Some(layer) = layers.layer_mut(st_layer) {
            if let Some(sprite) = layer.sprite_mut(sid) {
                sprite.visible = true;
                sprite.alpha = 255;
                sprite.fit = SpriteFit::PixelRect;
                sprite.size_mode = SpriteSizeMode::Intrinsic;
                sprite.x = 0;
                sprite.y = 0;
                sprite.order = 0;
            }
        }

        obj.layer_id = Some(st_layer);
        obj.sprite_id = Some(sid);
        Ok((st_layer, sid))
    }

    fn sync_object_sprite(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: usize,
        obj_idx: usize,
    ) -> Result<()> {
        let obj = self.ensure_object_mut(stage, obj_idx).clone();

        if obj.is_bg {
            let bg = layers.bg_mut();
            bg.visible = obj.disp;
            bg.x = obj.x as i32;
            bg.y = obj.y as i32;
            bg.alpha = obj.alpha.clamp(0, 255) as u8;
            bg.fit = SpriteFit::FullScreen;
            bg.size_mode = SpriteSizeMode::Intrinsic;

            if let Some(file) = &obj.file {
                let img_id = Self::load_any_image(images, file, obj.patno)?;
                bg.image_id = Some(img_id);
            }
            return Ok(());
        }

        let (lid, sid) = self.ensure_bound_sprite(layers, stage, obj_idx)?;
        let sprite = layers
            .layer_mut(lid)
            .and_then(|l| l.sprite_mut(sid))
            .context("sprite not found")?;

        sprite.visible = obj.disp;
        sprite.x = obj.x as i32;
        sprite.y = obj.y as i32;
        sprite.alpha = obj.alpha.clamp(0, 255) as u8;

        // Order: stage layer_no is treated as a coarse z, order as fine z.
        let coarse = obj.layer_no.clamp(-10000, 10000) as i32;
        let fine = obj.order.clamp(-100000, 100000) as i32;
        sprite.order = coarse.saturating_mul(1000).saturating_add(fine);

        if let Some(file) = &obj.file {
            let img_id = Self::load_any_image(images, file, obj.patno)?;
            sprite.image_id = Some(img_id);
        }

        Ok(())
    }

    pub fn stage_clear(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let len = self.stages[stage_u].objects.len();
        for idx in 0..len {
            {
                let obj = self.ensure_object_mut(stage_u, idx);
                obj.disp = false;
            }
            let _ = self.sync_object_sprite(images, layers, stage_u, idx);
        }
        Ok(())
    }

    pub fn object_create(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        file: &str,
        disp: i64,
        x: i64,
        y: i64,
        patno: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) {
            bail!("invalid stage: {stage}");
        }
        if obj_idx < 0 {
            bail!("invalid obj idx: {obj_idx}");
        }

        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let current_layer = self.current_layer;

        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.is_bg = stage_u == 0 && obj_u == 0;
            obj.file = Some(file.to_string());
            obj.patno = patno;
            obj.disp = disp != 0;
            obj.x = x;
            obj.y = y;

            // Default layer number from current selection (can be overridden by scripts).
            if obj.layer_no == 0 {
                obj.layer_no = current_layer as i64;
            }
        }

        // Ensure render binding exists for non-bg.
        if !(stage_u == 0 && obj_u == 0) {
            let _ = self.ensure_bound_sprite(layers, stage_u, obj_u)?;
        }

        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_disp(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        disp: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.disp = disp != 0;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_pos(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        x: i64,
        y: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let current_layer = self.current_layer;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.x = x;
            obj.y = y;
            if obj.layer_no == 0 {
                obj.layer_no = current_layer as i64;
            }
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_x(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        x: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.x = x;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_y(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        y: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.y = y;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_patno(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        patno: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.patno = patno;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_layer(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        layer_no: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.layer_no = layer_no;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_order(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        order: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.order = order;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_alpha(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        alpha: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.alpha = alpha;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_z(&mut self, stage: i64, obj_idx: i64, z: i64) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.ensure_object_mut(stage_u, obj_u);
        obj.z = z;
        Ok(())
    }

    pub fn object_clear(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.file = None;
            obj.patno = 0;
            obj.disp = false;
            obj.alpha = 255;
        }
        let _ = self.sync_object_sprite(images, layers, stage_u, obj_u);
        Ok(())
    }

    pub fn clear_objects_in_layer_no(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        layer_no: i64,
    ) -> Result<()> {
        for stage in 0..3usize {
            let len = self.stages[stage].objects.len();
            for obj_idx in 0..len {
                let matches = self
                    .object(stage, obj_idx)
                    .map(|o| o.layer_no == layer_no)
                    .unwrap_or(false);
                if !matches {
                    continue;
                }
                {
                    let obj = self.ensure_object_mut(stage, obj_idx);
                    obj.disp = false;
                }
                let _ = self.sync_object_sprite(images, layers, stage, obj_idx);
            }
        }
        Ok(())
    }

    pub fn object_get_pos(&self, stage: i64, obj_idx: i64) -> Option<(i64, i64)> {
        self.object_peek_pos(stage, obj_idx)
    }

    pub fn object_get_disp(&self, stage: i64, obj_idx: i64) -> Option<bool> {
        self.object_peek_disp(stage, obj_idx).map(|v| v != 0)
    }

    pub fn object_get_patno(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        self.object_peek_patno(stage, obj_idx)
    }

    pub fn object_get_layer(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        self.object_peek_layer(stage, obj_idx)
    }

    pub fn object_get_order(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        self.object_peek_order(stage, obj_idx)
    }

    pub fn object_get_alpha(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        self.object_peek_alpha(stage, obj_idx)
    }

    pub fn object_set_pat_no(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        patno: i64,
    ) -> Result<()> {
        self.object_set_patno(images, layers, stage, obj_idx, patno)
    }

    pub fn object_peek_pos(&self, stage: i64, obj_idx: i64) -> Option<(i64, i64)> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        Some((obj.x, obj.y))
    }

    pub fn object_peek_disp(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        Some(if obj.disp { 1 } else { 0 })
    }

    pub fn object_peek_patno(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        Some(obj.patno)
    }

    pub fn object_peek_layer(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        Some(obj.layer_no)
    }

    pub fn object_peek_order(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        Some(obj.order)
    }

    pub fn object_peek_alpha(&self, stage: i64, obj_idx: i64) -> Option<i64> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        Some(obj.alpha)
    }
}
