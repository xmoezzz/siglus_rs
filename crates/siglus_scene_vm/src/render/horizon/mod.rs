//! Nintendo Switch renderer: the desktop renderer (`render/mod.rs`) with
//! deko3d in place of wgpu. The frame is planned by `crate::render_plan`
//! and drawn with the desktop's own WGSL shaders, translated to GLSL by
//! platform/switch/shaderc; render targets have the logical screen size
//! and textures their full size with mip chains, as on the desktop, so the
//! same shaders see the same values. The GPU calls are the runtime's
//! (platform/switch/runtime/source/gpu.c).

mod gpu;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::assets::RgbaImage;
use crate::emote::EmoteRenderPacket;
use crate::image_manager::{ImageHandle, ImageKey, ImageManager};
use crate::layer::{RenderFrame, RenderSprite, SpriteBlend, WipeRenderPlan};
use crate::render_plan::emote::{EmoteTexture, EmoteVertex, plan_emote_draws};
use crate::render_plan::{
    BoneUniform, DrawCommand, FramePlanner, PageWipeVertex, PlanTarget, SurfaceViewport,
    TechniqueSpecial, Vertex, VertexSprite2dData, VsUniform, WipeMaskCacheKey, WipeUniform,
    build_page_wipe_draws, generated_wipe_mask, plain_sprite2d_uniform, wipe_uniform,
};
use crate::runtime::FrameCaptureBackend;

use gpu::{BlendState, GpuDraw, Texture, Uniform};

/// The desktop's shadow map side.
const SHADOW_MAP_SIDE: u32 = 2048;

/// `VertexSprite2dData` (the `VsIn2d` locations).
const SPRITE_ATTRIBUTES: &[(usize, u32, u32)] = &[
    (0, 3, 0),
    (1, 2, 12),
    (2, 2, 20),
    (3, 1, 28),
    (4, 4, 32),
    (5, 4, 48),
];
/// `Vertex` (`Vertex::ATTRS`, the `VsIn` locations).
const MESH_ATTRIBUTES: &[(usize, u32, u32)] = &[
    (0, 3, 0),
    (1, 2, 12),
    (2, 1, 28),
    (3, 4, 144),
    (4, 4, 160),
    (5, 4, 224),
    (6, 4, 240),
    (7, 4, 256),
    (8, 4, 288),
    (9, 4, 304),
];
const PAGE_ATTRIBUTES: &[(usize, u32, u32)] = &[(0, 4, 0), (1, 2, 16)];
/// `EmoteVertex`.
const EMOTE_ATTRIBUTES: &[(usize, u32, u32)] = &[
    (0, 2, 0),
    (1, 2, 8),
    (2, 2, 16),
    (3, 4, 24),
    (4, 1, 40),
    (5, 4, 44),
    (6, 3, 60),
];

/// The translated programs (`<name>.dksh` in the RomFS).
struct Programs {
    sprite_v: u32,
    sprite_f: u32,
    mesh_v: u32,
    mesh_f: u32,
    shadow_v: u32,
    shadow_f: u32,
    wipe_v: u32,
    wipe_f: u32,
    page_v: u32,
    page_f: u32,
    emote_v: u32,
    emote_f: u32,
}

impl Programs {
    fn load() -> Programs {
        Programs {
            sprite_v: gpu::program("sprite_vsh"),
            sprite_f: gpu::program("sprite_fsh"),
            mesh_v: gpu::program("mesh_vsh"),
            mesh_f: gpu::program("mesh_fsh"),
            shadow_v: gpu::program("shadow_vsh"),
            shadow_f: gpu::program("shadow_fsh"),
            wipe_v: gpu::program("wipe_vsh"),
            wipe_f: gpu::program("wipe_fsh"),
            page_v: gpu::program("page_vsh"),
            page_f: gpu::program("page_fsh"),
            emote_v: gpu::program("emote_vsh"),
            emote_f: gpu::program("emote_fsh"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TexKey {
    Image(u32),
    External(PathBuf),
}

/// The desktop's internal targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tgt {
    SceneA = 0,
    SceneB = 1,
    WipeA = 2,
    WipeB = 3,
}

impl Tgt {
    fn opposite(self) -> Tgt {
        match self {
            Tgt::SceneA => Tgt::SceneB,
            _ => Tgt::SceneA,
        }
    }
}

/// A target's size and the logical screen in it (x, y, w, h).
#[derive(Debug, Clone, Copy)]
struct Geom {
    width: u32,
    height: u32,
    viewport: [f32; 4],
}

struct EmoteTarget {
    output: Texture,
    feedback: Texture,
    feedback_valid: bool,
    resources: HashMap<u32, Texture>,
    version: u64,
    hit_version: Option<u64>,
}

pub struct Renderer {
    programs: Programs,
    display_w: u32,
    display_h: u32,
    logical_w: u32,
    logical_h: u32,
    /// The logical screen on the display (letterboxed).
    screen: [f32; 4],
    wait_display_vsync: bool,
    plan: FramePlanner,
    sprite_verts: Vec<VertexSprite2dData>,
    textures: HashMap<TexKey, Texture>,
    white: Option<Texture>,
    /// Scene A/B and wipe A/B (made on first use, as on the desktop).
    targets: Vec<Texture>,
    shadow_map: Option<Texture>,
    wipe_mask: Option<(WipeMaskCacheKey, Texture)>,
    emotes: HashMap<u64, EmoteTarget>,
    in_frame: bool,
    /// Where to save the next frame (`dump_next_frame`).
    dump: Option<PathBuf>,
}

impl Renderer {
    pub fn new(_width: u32, _height: u32) -> Result<Self> {
        let (display_w, display_h) = gpu::display_size();
        let mut renderer = Self {
            programs: Programs::load(),
            display_w,
            display_h,
            logical_w: display_w,
            logical_h: display_h,
            screen: [0.0, 0.0, display_w as f32, display_h as f32],
            wait_display_vsync: true,
            plan: FramePlanner::default(),
            sprite_verts: Vec::new(),
            textures: HashMap::new(),
            white: None,
            targets: Vec::new(),
            shadow_map: None,
            wipe_mask: None,
            emotes: HashMap::new(),
            in_frame: false,
            dump: None,
        };
        renderer.resize(display_w, display_h);
        Ok(renderer)
    }

    pub fn adapter_name(&self) -> String {
        "Nintendo Switch deko3d renderer".to_owned()
    }

    /// Sets the logical screen size; it is fitted into the display.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        let scale = (self.display_w as f64 / width as f64).min(self.display_h as f64 / height as f64);
        let draw_w = (width as f64 * scale).round().max(1.0);
        let draw_h = (height as f64 * scale).round().max(1.0);
        let screen = [
            ((self.display_w as f64 - draw_w) / 2.0).floor() as f32,
            ((self.display_h as f64 - draw_h) / 2.0).floor() as f32,
            draw_w as f32,
            draw_h as f32,
        ];
        if (width, height) != (self.logical_w, self.logical_h) || screen != self.screen {
            self.logical_w = width;
            self.logical_h = height;
            self.screen = screen;
            self.targets.clear();
            self.wipe_mask = None;
        }
    }

    pub fn resize_with_scale(&mut self, width: u32, height: u32, _scale_factor: f32) {
        self.resize(width, height);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resize_with_logical_viewport(
        &mut self,
        _surface_width: u32,
        _surface_height: u32,
        _scale_factor: f32,
        logical_width: u32,
        logical_height: u32,
        _viewport_x: u32,
        _viewport_y: u32,
        _viewport_width: u32,
        _viewport_height: u32,
    ) {
        self.resize(logical_width, logical_height);
    }

    pub fn set_wait_display_vsync(&mut self, wait_display_vsync: bool) {
        self.wait_display_vsync = wait_display_vsync;
    }

    pub fn clear_runtime_image_textures(&mut self) {
        self.textures.clear();
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (self.logical_w, self.logical_h)
    }

    /// The logical screen on the display: x, y, width, height in display
    /// pixels (touch input is mapped through it).
    pub fn screen_viewport(&self) -> [f32; 4] {
        self.screen
    }

    /// Saves the next frame (drawn through a render target at the logical
    /// size) as a PNG at `path`.
    pub fn dump_next_frame(&mut self, path: PathBuf) {
        self.dump = Some(path);
    }

    fn begin(&mut self) -> bool {
        if !gpu::begin_frame() {
            return false;
        }
        self.in_frame = true;
        if self.white.is_none() {
            let pixel = RgbaImage {
                width: 1,
                height: 1,
                center_x: 0,
                center_y: 0,
                rgba: vec![255; 4],
            };
            // The first texture: gpu.c binds id 0 where none is given.
            self.white = Texture::from_image(&pixel, (0, 0));
        }
        true
    }

    fn end(&mut self, present: bool) {
        gpu::end_frame(present);
        self.in_frame = false;
    }

    pub fn render_frame(&mut self, images: &ImageManager, frame: &RenderFrame) -> Result<()> {
        if !self.begin() {
            return Ok(());
        }
        self.prepare_emotes(frame);
        // As on the desktop: only frames that sample the drawn scene go
        // through the internal targets.
        let dump = self.dump.take();
        let needs_scene_texture = dump.is_some()
            || frame.wipe.is_some()
            || frame
                .sprites
                .iter()
                .any(|entry| matches!(entry.sprite.blend, SpriteBlend::Overlay));
        if needs_scene_texture {
            let final_target = self.render_frame_to_targets(images, frame)?;
            if let Some(path) = dump {
                let target = &self.targets[final_target as usize];
                let saved = match target.read() {
                    Some(rgba) => image::save_buffer(
                        &path,
                        &rgba,
                        target.width,
                        target.height,
                        image::ColorType::Rgba8,
                    )
                    .map_err(|err| err.to_string()),
                    None => Err("read failed".to_string()),
                };
                crate::switch_host::report_switch_diagnostic(&format!(
                    "siglus_switch: dump {} {saved:?} wipe={:?}\n",
                    path.display(),
                    frame.wipe.as_ref().map(|w| (w.wipe_type, w.progress, w.under.len(), w.current.len(), w.next.len(), w.over.len()))
                ));
                for (index, target) in self.targets.iter().enumerate() {
                    if let Some(rgba) = target.read() {
                        let _ = image::save_buffer(
                            path.with_extension(format!("t{index}.png")),
                            &rgba,
                            target.width,
                            target.height,
                            image::ColorType::Rgba8,
                        );
                    }
                }
            }
            let display = self.display_geom();
            gpu::begin_pass(None, Some([0.0, 0.0, 0.0, 1.0]), true);
            let source = self.targets[final_target as usize].id;
            self.copy(source, display);
        } else {
            self.render_to_display(images, &frame.sprites)?;
        }
        self.end(true);
        Ok(())
    }

    fn display_geom(&self) -> Geom {
        Geom {
            width: self.display_w,
            height: self.display_h,
            viewport: self.screen,
        }
    }

    fn target_geom(&self) -> Geom {
        Geom {
            width: self.logical_w,
            height: self.logical_h,
            viewport: [0.0, 0.0, self.logical_w as f32, self.logical_h as f32],
        }
    }

    fn ensure_targets(&mut self) -> Result<()> {
        while self.targets.len() < 4 {
            let target = Texture::target(self.logical_w, self.logical_h)
                .ok_or_else(|| anyhow::anyhow!("render target allocation failed"))?;
            self.targets.push(target);
        }
        Ok(())
    }

    /// `render_ordinary_frame_to_surface`.
    fn render_to_display(&mut self, images: &ImageManager, sprites: &[RenderSprite]) -> Result<()> {
        let geom = self.display_geom();
        self.plan_sprites(images, sprites, geom)?;
        self.render_shadow_pass(images, [1.0; 4]);
        gpu::begin_pass(None, Some([0.0, 0.0, 0.0, 1.0]), true);
        for index in 0..self.plan.draws.len() {
            self.draw_command(images, index, geom, None);
        }
        Ok(())
    }

    /// `render_frame_to_internal`.
    fn render_frame_to_targets(&mut self, images: &ImageManager, frame: &RenderFrame) -> Result<Tgt> {
        self.ensure_targets()?;
        let Some(wipe) = frame.wipe.as_ref() else {
            return self.render_list(images, &frame.sprites, Tgt::SceneA, false);
        };
        let current = self.render_list(images, &wipe.current, Tgt::WipeA, false)?;
        if current != Tgt::WipeA {
            self.copy_target(current, Tgt::WipeA);
        }
        let next = self.render_list(images, &wipe.next, Tgt::WipeB, false)?;
        if next != Tgt::WipeB {
            self.copy_target(next, Tgt::WipeB);
        }
        let under = self.render_list(images, &wipe.under, Tgt::SceneA, false)?;
        let composed = self.render_wipe_composite(images, wipe, under)?;
        self.render_list(images, &wipe.over, composed, true)
    }

    /// `render_sprite_list_to_scene_pair`.
    fn render_list(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
        start: Tgt,
        load: bool,
    ) -> Result<Tgt> {
        let geom = self.target_geom();
        self.plan_sprites(images, sprites, geom)?;
        self.render_shadow_pass(images, [0.0, 0.0, 0.0, 1.0]);
        let mut current = start;
        gpu::begin_pass(
            Some(&self.targets[current as usize]),
            (!load).then_some([0.0, 0.0, 0.0, 1.0]),
            true,
        );
        let count = self.plan.draws.len();
        let mut index = 0;
        while index < count {
            let overlay = is_overlay(&self.plan.draws[index]);
            let first = index;
            while index < count && is_overlay(&self.plan.draws[index]) == overlay {
                index += 1;
            }
            if overlay {
                let backdrop = if matches!(current, Tgt::SceneA | Tgt::SceneB) {
                    current
                } else {
                    self.copy_target(current, Tgt::SceneA);
                    Tgt::SceneA
                };
                let dst = backdrop.opposite();
                gpu::begin_pass(Some(&self.targets[dst as usize]), None, false);
                self.copy(self.targets[backdrop as usize].id, geom);
                for draw in first..index {
                    self.draw_command(images, draw, geom, Some(backdrop));
                }
                current = dst;
            } else {
                for draw in first..index {
                    self.draw_command(images, draw, geom, None);
                }
            }
        }
        Ok(current)
    }

    fn plan_sprites(&mut self, images: &ImageManager, sprites: &[RenderSprite], geom: Geom) -> Result<()> {
        self.plan.plan(
            images,
            sprites,
            &PlanTarget {
                win_w: self.logical_w as f32,
                win_h: self.logical_h as f32,
                surface_w: geom.width,
                surface_h: geom.height,
                surface_viewport: SurfaceViewport {
                    x: geom.viewport[0] as u32,
                    y: geom.viewport[1] as u32,
                    w: geom.viewport[2] as u32,
                    h: geom.viewport[3] as u32,
                },
            },
        )?;
        self.sprite_verts.clear();
        self.sprite_verts
            .extend(self.plan.verts.iter().map(|v| VertexSprite2dData::from(*v)));
        // Images the runtime released (`organize_textures`); gpu.c frees
        // their memory once the frames still reading them are done.
        self.textures.retain(|key, _| match key {
            TexKey::Image(index) => images.contains(ImageKey(*index)),
            TexKey::External(_) => true,
        });
        for index in 0..self.plan.draws.len() {
            let draw = &self.plan.draws[index];
            let handles = [
                draw.image_id.clone(),
                draw.mask_image_id.clone(),
                draw.tonecurve_image_id.clone(),
                draw.fog_image_id.clone(),
                draw.wipe_src_image_id.clone(),
            ];
            let paths = [
                draw.mesh_texture_path.clone(),
                draw.mesh_normal_texture_path.clone(),
                draw.mesh_toon_texture_path.clone(),
            ];
            for handle in handles.into_iter().flatten() {
                self.image_texture(images, &handle);
            }
            for path in paths.into_iter().flatten() {
                self.external_texture(&path);
            }
        }
        Ok(())
    }

    /// The texture of an image, uploaded or updated as needed.
    fn image_texture(&mut self, images: &ImageManager, handle: &ImageHandle) -> Option<i32> {
        let (image, version) = images.get_entry(handle)?;
        let key = TexKey::Image(handle.key().0);
        let source = (Arc::as_ptr(&image) as usize, version);
        if let Some(texture) = self.textures.get_mut(&key) {
            if texture.source == source {
                return Some(texture.id);
            }
            // gpu.c orders uploads with the draws, so a texture drawn
            // earlier this frame can be rewritten in place.
            if texture.width == image.width.max(1) && texture.height == image.height.max(1) {
                texture.write(&image);
                texture.source = source;
                return Some(texture.id);
            }
        }
        let texture = Texture::from_image(&image, source)?;
        let id = texture.id;
        self.textures.insert(key, texture);
        Some(id)
    }

    fn external_texture(&mut self, path: &PathBuf) -> Option<i32> {
        let key = TexKey::External(path.clone());
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.id);
        }
        let image = crate::assets::load_image_any(path, 0).ok()?;
        let texture = Texture::from_image(&image, (0, 0))?;
        let id = texture.id;
        self.textures.insert(key, texture);
        Some(id)
    }

    fn texture_of(&self, handle: &Option<ImageHandle>) -> i32 {
        handle
            .as_ref()
            .and_then(|h| self.textures.get(&TexKey::Image(h.key().0)))
            .map_or(-1, |t| t.id)
    }

    fn external_of(&self, path: &Option<PathBuf>) -> Option<&Texture> {
        path.as_ref()
            .and_then(|p| self.textures.get(&TexKey::External(p.clone())))
    }

    /// `render_command_slice` into the shadow map for the shadow casters.
    fn render_shadow_pass(&mut self, images: &ImageManager, clear: [f32; 4]) {
        let casters: Vec<usize> = (0..self.plan.draws.len())
            .filter(|&i| self.plan.draws[i].shadow_cast)
            .collect();
        if casters.is_empty() {
            return;
        }
        if self.shadow_map.is_none() {
            self.shadow_map = Texture::target(SHADOW_MAP_SIDE, SHADOW_MAP_SIDE);
        }
        let Some(shadow) = self.shadow_map.as_ref() else {
            return;
        };
        gpu::begin_pass(Some(shadow), Some(clear), true);
        let geom = Geom {
            width: SHADOW_MAP_SIDE,
            height: SHADOW_MAP_SIDE,
            viewport: [0.0, 0.0, SHADOW_MAP_SIDE as f32, SHADOW_MAP_SIDE as f32],
        };
        for index in casters {
            self.draw_mesh(images, index, geom, true);
        }
    }

    /// One draw command of the plan (`render_command_slice`).
    fn draw_command(&self, images: &ImageManager, index: usize, geom: Geom, backdrop: Option<Tgt>) {
        let cmd = &self.plan.draws[index];
        if !cmd.pipeline_key.program.uses_sprite2d_layout() {
            self.draw_mesh(images, index, geom, false);
            return;
        }
        let base = match cmd.emote_render_id {
            Some(render_id) => self.emotes.get(&render_id).map_or(-1, |t| t.output.id),
            None => self.texture_of(&cmd.image_id),
        };
        if base < 0 {
            return;
        }
        let aux = if matches!(cmd.pipeline_key.technique.special, TechniqueSpecial::Overlay) {
            backdrop.map_or(-1, |t| self.targets[t as usize].id)
        } else {
            self.texture_of(&cmd.wipe_src_image_id)
        };
        let mut draw = GpuDraw::new(self.programs.sprite_v, self.programs.sprite_f);
        draw.layout(size_of::<VertexSprite2dData>() as u32, SPRITE_ATTRIBUTES);
        let range = cmd.range.start as usize..cmd.range.end as usize;
        draw.vertices(&self.sprite_verts[range]);
        draw.textures[..5].copy_from_slice(&[
            base,
            self.texture_of(&cmd.mask_image_id),
            self.texture_of(&cmd.tonecurve_image_id),
            aux,
            self.texture_of(&cmd.fog_image_id),
        ]);
        self.apply_pipeline(&mut draw, cmd, geom);
        draw.fragment_uniforms[0] = Uniform::of(&cmd.vs_uniform);
        gpu::draw(&draw);
    }

    /// Blending, depth, culling, viewport and scissor of a command's
    /// pipeline key (`ensure_pipeline`).
    fn apply_pipeline(&self, draw: &mut GpuDraw, cmd: &DrawCommand, geom: Geom) {
        let key = &cmd.pipeline_key;
        draw.blend = if key.alpha_blend {
            BlendState::sprite(key.blend)
        } else {
            BlendState::NONE
        };
        if key.use_depth {
            draw.depth_test = 1;
            draw.depth_compare = gpu::COMPARE_LEQUAL;
            draw.depth_write = u8::from(key.depth_write);
        }
        // D3D's D3DCULL_CCW: clockwise triangles are the front.
        if key.cull_back {
            draw.cull_mode = gpu::FACE_BACK;
            draw.front_face = gpu::FRONT_CW;
        }
        draw.viewport = geom.viewport;
        draw.scissor = match cmd.scissor {
            Some(s) => [s.x as i32, s.y as i32, s.w as i32, s.h as i32],
            None => [
                geom.viewport[0] as i32,
                geom.viewport[1] as i32,
                geom.viewport[2] as i32,
                geom.viewport[3] as i32,
            ],
        };
    }

    /// A 3D draw: `vs_common`/`fs_common`, or the shadow programs.
    fn draw_mesh(&self, images: &ImageManager, index: usize, geom: Geom, shadow_pass: bool) {
        let _ = images;
        let cmd = &self.plan.draws[index];
        let (vertex, fragment) = if shadow_pass {
            (self.programs.shadow_v, self.programs.shadow_f)
        } else {
            (self.programs.mesh_v, self.programs.mesh_f)
        };
        let mut draw = GpuDraw::new(vertex, fragment);
        draw.layout(size_of::<Vertex>() as u32, MESH_ATTRIBUTES);
        let range = cmd.range.start as usize..cmd.range.end as usize;
        draw.vertices(&self.plan.verts[range]);
        let base = self
            .external_of(&cmd.mesh_texture_path)
            .map_or_else(|| self.texture_of(&cmd.image_id), |t| t.id);
        draw.textures = [
            base,
            self.texture_of(&cmd.mask_image_id),
            self.texture_of(&cmd.tonecurve_image_id),
            self.texture_of(&cmd.wipe_src_image_id),
            self.texture_of(&cmd.fog_image_id),
            self.external_of(&cmd.mesh_normal_texture_path).map_or(-1, |t| t.id),
            self.external_of(&cmd.mesh_toon_texture_path).map_or(-1, |t| t.id),
            if shadow_pass {
                -1
            } else {
                self.shadow_map.as_ref().map_or(-1, |t| t.id)
            },
        ];
        draw.samplers[7] = gpu::SAMPLER_POINT;
        self.apply_pipeline(&mut draw, cmd, geom);
        if shadow_pass {
            draw.blend = BlendState::NONE;
            draw.depth_test = 1;
            draw.depth_write = 1;
            draw.depth_compare = gpu::COMPARE_LEQUAL;
            draw.scissor = [0, 0, geom.width as i32, geom.height as i32];
        }
        static ZERO_BONES: std::sync::OnceLock<Box<BoneUniform>> = std::sync::OnceLock::new();
        let bones: &BoneUniform = match cmd.bone_uniform_index {
            Some(i) => self
                .plan
                .draw_bone_uniforms
                .get(i as usize)
                .unwrap_or_else(|| ZERO_BONES.get_or_init(|| Box::new(BoneUniform::zero()))),
            None => ZERO_BONES.get_or_init(|| Box::new(BoneUniform::zero())),
        };
        draw.vertex_uniforms = [Uniform::of(&cmd.vs_uniform), Uniform::of(bones)];
        draw.fragment_uniforms[0] = Uniform::of(&cmd.vs_uniform);
        gpu::draw(&draw);
    }

    /// `render_copy_pass`: `source` over the viewport, unblended.
    fn copy(&self, source: i32, geom: Geom) {
        let uniform: VsUniform = plain_sprite2d_uniform(geom.width as f32, geom.height as f32);
        let quad = fullscreen_quad();
        let mut draw = GpuDraw::new(self.programs.sprite_v, self.programs.sprite_f);
        draw.layout(size_of::<VertexSprite2dData>() as u32, SPRITE_ATTRIBUTES);
        draw.vertices(&quad);
        draw.textures[0] = source;
        draw.viewport = geom.viewport;
        draw.scissor = [
            geom.viewport[0] as i32,
            geom.viewport[1] as i32,
            geom.viewport[2] as i32,
            geom.viewport[3] as i32,
        ];
        draw.fragment_uniforms[0] = Uniform::of(&uniform);
        gpu::draw(&draw);
    }

    fn copy_target(&self, src: Tgt, dst: Tgt) {
        gpu::begin_pass(Some(&self.targets[dst as usize]), None, false);
        self.copy(self.targets[src as usize].id, self.target_geom());
    }

    /// `render_wipe_composite`, and the page wipes.
    fn render_wipe_composite(&mut self, images: &ImageManager, wipe: &WipeRenderPlan, under: Tgt) -> Result<Tgt> {
        let target = if matches!(under, Tgt::SceneA | Tgt::SceneB) {
            under.opposite()
        } else {
            Tgt::SceneB
        };
        let geom = self.target_geom();
        if matches!(wipe.wipe_type, 300 | 301) {
            gpu::begin_pass(Some(&self.targets[target as usize]), None, true);
            self.copy(self.targets[under as usize].id, geom);
            for page in build_page_wipe_draws(wipe, self.logical_w as f32, self.logical_h as f32) {
                let mut draw = GpuDraw::new(self.programs.page_v, self.programs.page_f);
                draw.layout(size_of::<PageWipeVertex>() as u32, PAGE_ATTRIBUTES);
                draw.vertices(&page.vertices);
                let source = if page.use_current { Tgt::WipeA } else { Tgt::WipeB };
                draw.textures[0] = self.targets[source as usize].id;
                draw.blend = BlendState::ALPHA;
                draw.depth_test = 1;
                draw.depth_write = 1;
                draw.depth_compare = gpu::COMPARE_LEQUAL;
                draw.cull_mode = gpu::FACE_BACK;
                draw.front_face = gpu::FRONT_CCW;
                draw.viewport = geom.viewport;
                draw.scissor = [0, 0, geom.width as i32, geom.height as i32];
                gpu::draw(&draw);
            }
            return Ok(target);
        }
        let mask = match wipe.mask_image_id.as_ref() {
            Some(id) => self.image_texture(images, id).unwrap_or(-1),
            None => self.generated_wipe_mask(wipe),
        };
        let uniform: WipeUniform = wipe_uniform(wipe, self.logical_w as f32, self.logical_h as f32);
        gpu::begin_pass(Some(&self.targets[target as usize]), Some([0.0, 0.0, 0.0, 1.0]), false);
        // `vs_main` builds its full-screen triangle from the vertex index.
        let dummy = [0u32; 3];
        let mut draw = GpuDraw::new(self.programs.wipe_v, self.programs.wipe_f);
        draw.stride = 4;
        draw.vertices(&dummy);
        draw.textures[..4].copy_from_slice(&[
            self.targets[under as usize].id,
            self.targets[Tgt::WipeA as usize].id,
            self.targets[Tgt::WipeB as usize].id,
            mask,
        ]);
        draw.viewport = geom.viewport;
        draw.scissor = [0, 0, geom.width as i32, geom.height as i32];
        draw.fragment_uniforms[0] = Uniform::of(&uniform);
        gpu::draw(&draw);
        Ok(target)
    }

    /// `ensure_generated_wipe_mask`.
    fn generated_wipe_mask(&mut self, wipe: &WipeRenderPlan) -> i32 {
        let key = WipeMaskCacheKey {
            wipe_type: wipe.wipe_type,
            option: wipe.option.clone(),
            width: self.logical_w,
            height: self.logical_h,
            seed: wipe.random_seed,
        };
        if let Some((cached, texture)) = &self.wipe_mask
            && cached == &key
        {
            return texture.id;
        }
        self.wipe_mask = generated_wipe_mask(wipe, self.logical_w, self.logical_h)
            .and_then(|image| Texture::from_image(&image, (0, 0)))
            .map(|texture| (key, texture));
        self.wipe_mask.as_ref().map_or(-1, |(_, t)| t.id)
    }

    /// Composes the frame's changed E-mote objects into their targets
    /// (`EmoteCompositor::prepare`).
    fn prepare_emotes(&mut self, frame: &RenderFrame) {
        let lists: Vec<&[RenderSprite]> = match frame.wipe.as_ref() {
            Some(wipe) => vec![&wipe.under, &wipe.current, &wipe.next, &wipe.over],
            None => vec![&frame.sprites],
        };
        let packets: Vec<&EmoteRenderPacket> = lists
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.sprite.emote_render.as_deref())
            .collect();
        let live: HashSet<u64> = packets.iter().map(|packet| packet.render_id).collect();
        self.emotes.retain(|id, _| live.contains(id));
        for packet in packets {
            self.prepare_emote(packet);
        }
    }

    fn prepare_emote(&mut self, packet: &EmoteRenderPacket) {
        let (width, height) = (packet.width.max(1), packet.height.max(1));
        let recreate = self
            .emotes
            .get(&packet.render_id)
            .is_none_or(|t| t.output.width != width || t.output.height != height);
        if recreate {
            let (Some(output), Some(feedback)) = (Texture::target(width, height), Texture::target(width, height)) else {
                return;
            };
            let resources = packet
                .textures
                .iter()
                .filter_map(|(&index, source)| {
                    let image = RgbaImage {
                        width: source.width,
                        height: source.height,
                        center_x: 0,
                        center_y: 0,
                        rgba: source.rgba.as_ref().clone(),
                    };
                    Texture::from_image(&image, (0, 0)).map(|t| (index, t))
                })
                .collect();
            self.emotes.insert(
                packet.render_id,
                EmoteTarget {
                    output,
                    feedback,
                    feedback_valid: false,
                    resources,
                    version: u64::MAX,
                    hit_version: None,
                },
            );
        }
        let Some(target) = self.emotes.get_mut(&packet.render_id) else {
            return;
        };
        if target.version != packet.version {
            // The previous composition becomes the feedback texture (the
            // desktop copies it there).
            std::mem::swap(&mut target.output, &mut target.feedback);
            target.feedback_valid = target.version != u64::MAX;
            target.version = packet.version;
            self.draw_emote(packet);
        }
        let Some(target) = self.emotes.get_mut(&packet.render_id) else {
            return;
        };
        if packet.alpha_readback && !packet.has_current_hit_surface() && target.hit_version != Some(packet.version) {
            target.hit_version = Some(packet.version);
            if let Some(rgba) = target.output.read() {
                packet.publish_hit_alpha(rgba.chunks_exact(4).map(|px| px[3]).collect());
            }
        }
    }

    fn draw_emote(&self, packet: &EmoteRenderPacket) {
        let Some(target) = self.emotes.get(&packet.render_id) else {
            return;
        };
        let viewport = [0.0, 0.0, target.output.width as f32, target.output.height as f32];
        let scissor = [0, 0, target.output.width as i32, target.output.height as i32];
        let resolve = |texture: EmoteTexture| -> Option<i32> {
            match texture {
                EmoteTexture::Resource(index) => target.resources.get(&index).map(|t| t.id),
                EmoteTexture::Feedback => target.feedback_valid.then_some(target.feedback.id),
            }
        };
        let emote_draw = |vertices: &[EmoteVertex], texture: i32, blend: BlendState| {
            let mut draw = GpuDraw::new(self.programs.emote_v, self.programs.emote_f);
            draw.layout(size_of::<EmoteVertex>() as u32, EMOTE_ATTRIBUTES);
            draw.vertices(vertices);
            draw.textures[0] = texture;
            // eng_emote.cpp samples the object's textures with POINT.
            draw.samplers[0] = gpu::SAMPLER_POINT;
            draw.blend = blend;
            draw.viewport = viewport;
            draw.scissor = scissor;
            draw
        };
        gpu::begin_pass(Some(&target.output), Some([0.0; 4]), true);
        for item in plan_emote_draws(packet) {
            let Some(texture) = resolve(item.texture) else {
                continue;
            };
            let mut color = emote_draw(&item.vertices, texture, BlendState::emote(item.blend_index));
            if !item.stencil_groups.is_empty() {
                gpu::clear_stencil(item.stencil_initial_reference as u8);
                let mut reference = item.stencil_initial_reference as u8;
                let final_reference = item.stencil_final_reference as u8;
                let masks = item
                    .stencil_groups
                    .iter()
                    .filter(|g| g.phase == 1)
                    .map(|g| {
                        let r = reference;
                        reference = reference.saturating_add(1);
                        (g, r, gpu::STENCIL_INCR_WRAP)
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .chain(
                        item.stencil_groups
                            .iter()
                            .filter(|g| g.phase == 2)
                            .map(|g| (g, final_reference, gpu::STENCIL_DECR_WRAP)),
                    );
                for (group, reference, op) in masks {
                    for source in &group.sources {
                        let Some(mask_texture) = resolve(source.texture) else {
                            continue;
                        };
                        let mut mask = emote_draw(&source.vertices, mask_texture, BlendState::NONE);
                        mask.color_write_mask = 0;
                        mask.stencil_enable = 1;
                        mask.stencil_compare = gpu::COMPARE_EQUAL;
                        mask.stencil_depth_fail = op;
                        mask.stencil_pass = op;
                        mask.stencil_ref = reference;
                        gpu::draw(&mask);
                    }
                }
                color.stencil_enable = 1;
                color.stencil_compare = gpu::COMPARE_EQUAL;
                color.stencil_ref = final_reference;
            }
            gpu::draw(&color);
        }
    }
}

fn is_overlay(cmd: &DrawCommand) -> bool {
    matches!(cmd.pipeline_key.technique.special, TechniqueSpecial::Overlay)
}

/// Two triangles covering the viewport, texture rows top-down.
fn fullscreen_quad() -> [VertexSprite2dData; 6] {
    let v = |x: f32, y: f32, u: f32, t: f32| VertexSprite2dData {
        pos: [x, y, 0.0],
        uv: [u, t],
        uv_aux: [0.0, 0.0],
        alpha: 1.0,
        world_pos: [0.0; 4],
        world_normal: [0.0; 4],
    };
    [
        v(-1.0, 1.0, 0.0, 0.0),
        v(1.0, 1.0, 1.0, 0.0),
        v(1.0, -1.0, 1.0, 1.0),
        v(-1.0, 1.0, 0.0, 0.0),
        v(1.0, -1.0, 1.0, 1.0),
        v(-1.0, -1.0, 0.0, 1.0),
    ]
}

impl FrameCaptureBackend for Renderer {
    /// The frame drawn into the internal targets at the logical size and
    /// read back, as on the desktop.
    fn capture_render_frame(
        &mut self,
        images: &ImageManager,
        frame: &RenderFrame,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<RgbaImage> {
        let started = !self.in_frame;
        if started && !self.begin() {
            anyhow::bail!("GPU frame unavailable for capture");
        }
        self.prepare_emotes(frame);
        let final_target = self.render_frame_to_targets(images, frame)?;
        let target = &self.targets[final_target as usize];
        let rgba = target
            .read()
            .ok_or_else(|| anyhow::anyhow!("capture read-back failed"))?;
        let (tw, th) = (target.width as usize, target.height as usize);
        if started {
            self.end(false);
        }
        let width = logical_width.max(1) as usize;
        let height = logical_height.max(1) as usize;
        let mut out = vec![0u8; width * height * 4];
        for y in 0..height {
            let sy = (y * th / height).min(th - 1);
            for x in 0..width {
                let sx = (x * tw / width).min(tw - 1);
                out[(y * width + x) * 4..][..4].copy_from_slice(&rgba[(sy * tw + sx) * 4..][..4]);
                out[(y * width + x) * 4 + 3] = 255;
            }
        }
        Ok(RgbaImage {
            width: width as u32,
            height: height as u32,
            center_x: 0,
            center_y: 0,
            rgba: out,
        })
    }
}
