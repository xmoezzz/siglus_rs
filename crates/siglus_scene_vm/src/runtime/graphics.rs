//! Graphics runtime: bridges VM stage/object operations to `LayerManager` + `ImageManager`.
//!
//! This layer maps stage/object operations onto renderable sprites while preserving
//! stable sprite identities for the VM runtime.

use anyhow::{bail, Context, Result};

use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{
    ClipRect, LayerId, LayerManager, SpriteBlend, SpriteFit, SpriteId, SpriteSizeMode,
};

#[derive(Debug, Clone)]
struct ObjectState {
    is_bg: bool,

    // Render binding for non-BG objects.
    layer_id: Option<LayerId>,
    sprite_id: Option<SpriteId>,

    // Logical properties.
    is_mesh: bool,
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
    center_x: i64,
    center_y: i64,
    scale_x: i64,
    scale_y: i64,
    rotate_z: i64,
    clip_use: i64,
    clip_left: i64,
    clip_top: i64,
    clip_right: i64,
    clip_bottom: i64,
    src_clip_use: i64,
    src_clip_left: i64,
    src_clip_top: i64,
    src_clip_right: i64,
    src_clip_bottom: i64,
    tr: i64,
    mono: i64,
    reverse: i64,
    bright: i64,
    dark: i64,
    color_rate: i64,
    color_add_r: i64,
    color_add_g: i64,
    color_add_b: i64,
    color_r: i64,
    color_g: i64,
    color_b: i64,
    blend: i64,
    light_no: i64,
    fog_use: i64,
}

impl Default for ObjectState {
    fn default() -> Self {
        Self {
            is_bg: false,
            layer_id: None,
            sprite_id: None,
            is_mesh: false,
            file: None,
            patno: 0,
            disp: false,
            x: 0,
            y: 0,
            layer_no: 0,
            order: 0,
            alpha: 255,
            z: 0,
            center_x: 0,
            center_y: 0,
            scale_x: 1000,
            scale_y: 1000,
            rotate_z: 0,
            clip_use: 0,
            clip_left: 0,
            clip_top: 0,
            clip_right: 0,
            clip_bottom: 0,
            src_clip_use: 0,
            src_clip_left: 0,
            src_clip_top: 0,
            src_clip_right: 0,
            src_clip_bottom: 0,
            tr: 255,
            mono: 0,
            reverse: 0,
            bright: 0,
            dark: 0,
            color_rate: 0,
            color_add_r: 0,
            color_add_g: 0,
            color_add_b: 0,
            color_r: 0,
            color_g: 0,
            color_b: 0,
            blend: 0,
            light_no: -1,
            fog_use: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DebugObjectSpriteBinding {
    pub stage: usize,
    pub obj_idx: usize,
    pub is_bg: bool,
    pub layer_id: Option<LayerId>,
    pub sprite_id: Option<SpriteId>,
    pub file: Option<String>,
    pub patno: i64,
    pub disp: bool,
    pub x: i64,
    pub y: i64,
    pub layer_no: i64,
    pub order: i64,
    pub alpha: i64,
    pub z: i64,
    pub tr: i64,
    pub clip_use: i64,
    pub clip_left: i64,
    pub clip_top: i64,
    pub clip_right: i64,
    pub clip_bottom: i64,
    pub scale_x: i64,
    pub scale_y: i64,
    pub rotate_z: i64,
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
            stages: [
                StageState::default(),
                StageState::default(),
                StageState::default(),
            ],
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

    /// Expose stage layer allocation for non-Gfx backends (e.g., movie sprites).
    pub fn ensure_stage_layer_id(
        &mut self,
        layers: &mut LayerManager,
        stage: i64,
    ) -> Option<LayerId> {
        if stage < 0 || stage > 2 {
            return None;
        }
        Some(self.ensure_stage_layer(layers, stage as usize))
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

    pub fn debug_object_snapshot(
        &self,
        stage: usize,
        obj_idx: usize,
    ) -> Option<DebugObjectSpriteBinding> {
        let obj = self.object(stage, obj_idx)?;
        Some(DebugObjectSpriteBinding {
            stage,
            obj_idx,
            is_bg: obj.is_bg,
            layer_id: obj.layer_id,
            sprite_id: obj.sprite_id,
            file: obj.file.clone(),
            patno: obj.patno,
            disp: obj.disp,
            x: obj.x,
            y: obj.y,
            layer_no: obj.layer_no,
            order: obj.order,
            alpha: obj.alpha,
            z: obj.z,
            tr: obj.tr,
            clip_use: obj.clip_use,
            clip_left: obj.clip_left,
            clip_top: obj.clip_top,
            clip_right: obj.clip_right,
            clip_bottom: obj.clip_bottom,
            scale_x: obj.scale_x,
            scale_y: obj.scale_y,
            rotate_z: obj.rotate_z,
        })
    }

    pub fn object_sprite_binding(&self, stage: i64, obj_idx: i64) -> Option<(LayerId, SpriteId)> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let obj = self.object(stage_i as usize, obj_idx as usize)?;
        if obj.is_bg {
            return None;
        }
        match (obj.layer_id, obj.sprite_id) {
            (Some(lid), Some(sid)) => Some((lid, sid)),
            _ => None,
        }
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
            let layer = layers
                .layer_mut(st_layer)
                .context("stage layer not found")?;
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
            bg.scale_x = obj.scale_x as f32 / 1000.0;
            bg.scale_y = obj.scale_y as f32 / 1000.0;
            bg.rotate = obj.rotate_z as f32 * std::f32::consts::PI / 1800.0;
            bg.pivot_x = obj.center_x as f32;
            bg.pivot_y = obj.center_y as f32;
            bg.dst_clip = clip_rect(
                obj.clip_use,
                obj.clip_left,
                obj.clip_top,
                obj.clip_right,
                obj.clip_bottom,
            );
            bg.src_clip = clip_rect(
                obj.src_clip_use,
                obj.src_clip_left,
                obj.src_clip_top,
                obj.src_clip_right,
                obj.src_clip_bottom,
            );
            bg.tr = obj.tr.clamp(0, 255) as u8;
            bg.mono = obj.mono.clamp(0, 255) as u8;
            bg.reverse = obj.reverse.clamp(0, 255) as u8;
            bg.bright = obj.bright.clamp(0, 255) as u8;
            bg.dark = obj.dark.clamp(0, 255) as u8;
            bg.color_rate = obj.color_rate.clamp(0, 255) as u8;
            bg.color_add_r = obj.color_add_r.clamp(0, 255) as u8;
            bg.color_add_g = obj.color_add_g.clamp(0, 255) as u8;
            bg.color_add_b = obj.color_add_b.clamp(0, 255) as u8;
            bg.color_r = obj.color_r.clamp(0, 255) as u8;
            bg.color_g = obj.color_g.clamp(0, 255) as u8;
            bg.color_b = obj.color_b.clamp(0, 255) as u8;
            bg.blend = SpriteBlend::from_i64(obj.blend);
            bg.light_no = obj.light_no as i32;
            bg.fog_use = obj.fog_use != 0;

            if obj.is_mesh {
                bg.image_id = None;
                bg.mesh_file_name = obj.file.clone();
                bg.mesh_kind = 1;
                bg.camera_enabled = true;
            } else if let Some(file) = &obj.file {
                match Self::load_any_image(images, file, obj.patno) {
                    Ok(img_id) => bg.image_id = Some(img_id),
                    Err(err) if is_probable_mesh_path(file) => {
                        let _ = err;
                        bg.image_id = None;
                    }
                    Err(err) => return Err(err),
                }
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
        sprite.scale_x = obj.scale_x as f32 / 1000.0;
        sprite.scale_y = obj.scale_y as f32 / 1000.0;
        sprite.rotate = obj.rotate_z as f32 * std::f32::consts::PI / 1800.0;
        sprite.pivot_x = obj.center_x as f32;
        sprite.pivot_y = obj.center_y as f32;
        sprite.dst_clip = clip_rect(
            obj.clip_use,
            obj.clip_left,
            obj.clip_top,
            obj.clip_right,
            obj.clip_bottom,
        );
        sprite.src_clip = clip_rect(
            obj.src_clip_use,
            obj.src_clip_left,
            obj.src_clip_top,
            obj.src_clip_right,
            obj.src_clip_bottom,
        );
        sprite.tr = obj.tr.clamp(0, 255) as u8;
        sprite.mono = obj.mono.clamp(0, 255) as u8;
        sprite.reverse = obj.reverse.clamp(0, 255) as u8;
        sprite.bright = obj.bright.clamp(0, 255) as u8;
        sprite.dark = obj.dark.clamp(0, 255) as u8;
        sprite.color_rate = obj.color_rate.clamp(0, 255) as u8;
        sprite.color_add_r = obj.color_add_r.clamp(0, 255) as u8;
        sprite.color_add_g = obj.color_add_g.clamp(0, 255) as u8;
        sprite.color_add_b = obj.color_add_b.clamp(0, 255) as u8;
        sprite.color_r = obj.color_r.clamp(0, 255) as u8;
        sprite.color_g = obj.color_g.clamp(0, 255) as u8;
        sprite.color_b = obj.color_b.clamp(0, 255) as u8;
        sprite.blend = SpriteBlend::from_i64(obj.blend);
        sprite.light_no = obj.light_no as i32;
        sprite.fog_use = obj.fog_use != 0;

        // Order: stage layer_no is treated as a coarse z, order as fine z.
        let coarse = obj.layer_no.clamp(-10000, 10000) as i32;
        let fine = obj.order.clamp(-100000, 100000) as i32;
        sprite.order = coarse.saturating_mul(1000).saturating_add(fine);

        if obj.is_mesh {
            sprite.image_id = None;
            sprite.mesh_file_name = obj.file.clone();
            sprite.mesh_kind = 1;
            sprite.camera_enabled = true;
            sprite.shadow_cast = true;
            sprite.shadow_receive = true;
        } else if let Some(file) = &obj.file {
            match Self::load_any_image(images, file, obj.patno) {
                Ok(img_id) => sprite.image_id = Some(img_id),
                Err(err) if is_probable_mesh_path(file) => {
                    let _ = err;
                    sprite.image_id = None;
                }
                Err(err) => return Err(err),
            }
        }

        Ok(())
    }

    pub fn object_set_center(
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
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.center_x = x;
            obj.center_y = y;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_scale(
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
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.scale_x = x;
            obj.scale_y = y;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_rotate(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        z: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.rotate_z = z;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_clip(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        use_flag: i64,
        left: i64,
        top: i64,
        right: i64,
        bottom: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.clip_use = use_flag;
            obj.clip_left = left;
            obj.clip_top = top;
            obj.clip_right = right;
            obj.clip_bottom = bottom;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_src_clip(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        use_flag: i64,
        left: i64,
        top: i64,
        right: i64,
        bottom: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.src_clip_use = use_flag;
            obj.src_clip_left = left;
            obj.src_clip_top = top;
            obj.src_clip_right = right;
            obj.src_clip_bottom = bottom;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
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
            obj.is_mesh = false;
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

    pub fn object_create_mesh(
        &mut self,
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
            obj.is_mesh = true;
            obj.file = Some(file.to_string());
            obj.patno = patno;
            obj.disp = disp != 0;
            obj.x = x;
            obj.y = y;
            if obj.layer_no == 0 {
                obj.layer_no = current_layer as i64;
            }
        }

        if stage_u == 0 && obj_u == 0 {
            let bg = layers.bg_mut();
            bg.visible = disp != 0;
            bg.image_id = None;
            bg.x = x as i32;
            bg.y = y as i32;
        } else {
            let (lid, sid) = self.ensure_bound_sprite(layers, stage_u, obj_u)?;
            let sprite = layers
                .layer_mut(lid)
                .and_then(|l| l.sprite_mut(sid))
                .context("mesh sprite not found")?;
            sprite.visible = disp != 0;
            sprite.image_id = None;
            sprite.x = x as i32;
            sprite.y = y as i32;
            sprite.alpha = 255;
            sprite.tr = 255;
            sprite.fit = SpriteFit::PixelRect;
            sprite.size_mode = SpriteSizeMode::Intrinsic;
            sprite.mesh_file_name = Some(file.to_string());
            sprite.mesh_kind = 1;
            sprite.camera_enabled = true;
            sprite.shadow_cast = true;
            sprite.shadow_receive = true;
        }

        Ok(())
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

    pub fn object_set_tr(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        tr: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.tr = tr;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_mono(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        mono: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.mono = mono;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_reverse(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        reverse: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.reverse = reverse;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_bright(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        bright: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.bright = bright;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_dark(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        dark: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.dark = dark;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_color_rate(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        rate: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.color_rate = rate;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_color_add(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        r: i64,
        g: i64,
        b: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.color_add_r = r;
            obj.color_add_g = g;
            obj.color_add_b = b;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_color(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        r: i64,
        g: i64,
        b: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.color_r = r;
            obj.color_g = g;
            obj.color_b = b;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_blend(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        blend: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.blend = blend;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_light_no(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        light_no: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.light_no = light_no;
        }
        self.sync_object_sprite(images, layers, stage_u, obj_u)
    }

    pub fn object_set_fog_use(
        &mut self,
        images: &mut ImageManager,
        layers: &mut LayerManager,
        stage: i64,
        obj_idx: i64,
        fog_use: i64,
    ) -> Result<()> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return Ok(());
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        {
            let obj = self.ensure_object_mut(stage_u, obj_u);
            obj.fog_use = fog_use;
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

    pub fn object_peek_file(&self, stage: i64, obj_idx: i64) -> Option<String> {
        let stage_i = stage as isize;
        if !(0..3).contains(&stage_i) || obj_idx < 0 {
            return None;
        }
        let stage_u = stage_i as usize;
        let obj_u = obj_idx as usize;
        let obj = self.object(stage_u, obj_u)?;
        obj.file.clone()
    }
}

fn is_probable_mesh_path(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    lower.ends_with(".x")
        || lower.ends_with(".obj")
        || lower.ends_with(".fbx")
        || lower.ends_with(".gltf")
        || lower.ends_with(".glb")
}

fn clip_rect(use_flag: i64, left: i64, top: i64, right: i64, bottom: i64) -> Option<ClipRect> {
    if use_flag == 0 {
        return None;
    }
    Some(ClipRect {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    })
}
