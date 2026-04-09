//! Runtime scaffolding for command execution.
//!
//! This layer is intentionally pragmatic:
//! - It supports named commands used by standalone runtime tools.
//! - It supports numeric dispatch (forms) used by the VM.
//! - Unknown or unfinished operations are recorded instead of crashing.

pub mod commands;
pub mod forms;
pub mod graphics;
pub mod id_map;
pub mod input;
pub mod opcode;

pub use opcode::OpCode;
pub mod gan;
pub mod globals;
pub mod int_event;
pub mod net;
pub mod tables;
pub mod ui;
pub mod unknown;
pub mod wait;
use crate::runtime::forms::syscom as syscom_form;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

use crate::assets::RgbaImage;
use crate::audio::{AudioHub, BgmEngine, PcmEngine, SeEngine};
use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{
    ClipRect, LayerId, LayerManager, RenderSprite, Sprite, SpriteFit, SpriteId, SpriteSizeMode,
};
use crate::movie::MovieManager;
use crate::soft_render;
use crate::text_render::FontCache;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Str(String),
    /// An element chain as raw i32 codes (as stored on the VM int stack).
    Element(Vec<i32>),
    /// A nested list value (FM_LIST).
    List(Vec<Value>),
    /// A named argument (id -> value), used by some engine commands.
    NamedArg {
        id: i32,
        value: Box<Value>,
    },
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::NamedArg { value, .. } => value.as_i64(),
            _ => None,
        }
    }

    pub fn named_id(&self) -> Option<i32> {
        match self {
            Value::NamedArg { id, .. } => Some(*id),
            _ => None,
        }
    }

    pub fn unwrap_named(&self) -> &Value {
        match self {
            Value::NamedArg { value, .. } => value.as_ref(),
            _ => self,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            Value::NamedArg { value, .. } => value.as_str(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    /// Optional numeric code for VM forms.
    pub code: Option<opcode::OpCode>,
    pub args: Vec<Value>,
}

/// State used by a subset of EXCALL compatibility ops.
///
/// We intentionally keep these names offset-based instead of guessing their meaning.
#[derive(Debug, Default, Clone)]
pub struct ExcallCompatState {
    pub flag_204: bool,
    pub flag_2148: bool,
}

/// Optional external handler for numeric forms.
///
/// The project can keep game-specific implementations (e.g. SCREEN/MSGBK)
/// outside this crate, while still letting the VM dispatch through here.
pub trait ExternalFormHandler: Send + Sync {
    /// Return true if the form ID was handled.
    fn dispatch_form(
        &self,
        ctx: &mut CommandContext,
        form_id: u32,
        args: &[Value],
    ) -> anyhow::Result<bool>;
}

pub struct CommandContext {
    pub project_dir: PathBuf,

    pub images: ImageManager,
    pub layers: LayerManager,
    /// 1x1 white sprite used for screen-space overlays (filters, etc.).
    pub solid_white: ImageId,

    pub audio: AudioHub,

    pub bgm: BgmEngine,
    pub pcm: PcmEngine,
    pub se: SeEngine,

    pub movie: MovieManager,

    /// Runtime numeric id map (form/element/op codes). Defaults can be overridden.
    pub ids: id_map::IdMap,

    /// Minimal graphics runtime state for stage/object sprite binding.
    pub gfx: graphics::GfxRuntime,

    /// Minimal UI runtime (text window, message waits, etc.).
    pub ui: ui::UiRuntime,
    /// Shared font cache for stage/object text rendering.
    pub font_cache: FontCache,

    /// VM-visible input state (queried via INPUT/MOUSE/KEYLIST forms).
    pub input: input::InputState,

    /// Current render target size (used for UI layout).
    pub screen_w: u32,
    pub screen_h: u32,

    /// VM blocking state (WAIT / WAIT_KEY).
    pub wait: wait::VmWait,

    /// Lightweight network/browser helper mirroring the engine's `tnm_net` slot.
    pub net: net::TnmNet,

    /// Gameexe-driven asset tables (CGTABLE / DATABASE / THUMBTABLE).
    pub tables: tables::AssetTables,

    /// Value stack used by form handlers to return results.
    pub stack: Vec<Value>,

    pub unknown: unknown::UnknownOpRecorder,

    pub globals: globals::GlobalState,

    pub excall_state: ExcallCompatState,

    /// Optional project-provided form handler (game-specific).
    pub external_forms: Option<Arc<dyn ExternalFormHandler>>,
}

impl CommandContext {
    fn should_wheel_advance_message(&self) -> bool {
        const GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 305;
        self.globals
            .syscom
            .config_int
            .get(&GET_WHEEL_NEXT_MESSAGE_ONOFF)
            .copied()
            .unwrap_or(1)
            != 0
    }

    fn should_stop_koe_on_advance(&self) -> bool {
        const GET_KOE_DONT_STOP_ONOFF: i32 = 308;
        let syscom_dont_stop = self
            .globals
            .syscom
            .config_int
            .get(&GET_KOE_DONT_STOP_ONOFF)
            .copied()
            .unwrap_or(0)
            != 0;
        let script = &self.globals.script;
        let mut dont_stop = syscom_dont_stop || script.koe_dont_stop_on_flag;
        if script.koe_dont_stop_off_flag {
            dont_stop = false;
        }
        !dont_stop
    }

    fn advance_message_wait(&mut self, allow: bool) {
        if !allow || !self.ui.waiting_message {
            return;
        }
        self.ui.end_wait_message();
        if self.should_stop_koe_on_advance() {
            let _ = self.se.stop(None);
            let _ = self.pcm.stop_all(None);
        }
    }
    pub fn new(project_dir: PathBuf) -> Self {
        let mut unknown = unknown::UnknownOpRecorder::default();
        let tables = tables::AssetTables::load(&project_dir, &mut unknown);

        let mut ids = id_map::IdMap::load_from_env();
        // External configuration (repository file) is preferred over hard-coded defaults.
        ids.try_load_idmap_file(&project_dir.join("id_map.txt"));
        ids.try_load_idmap_file(&project_dir.join("siglus_id_map.txt"));
        if ids.user_cmd_names.is_empty() {
            ids.try_load_user_cmd_names_file(&project_dir.join("user_cmd_map.txt"));
            ids.try_load_user_cmd_names_file(&project_dir.join("siglus_user_cmd_map.txt"));
        }

        let audio = AudioHub::new();
        let mut images = ImageManager::new(project_dir.clone());
        let solid_white = images.solid_rgba((255, 255, 255, 255));

        Self {
            images,
            layers: LayerManager::default(),
            audio,
            bgm: BgmEngine::new(project_dir.clone()),
            pcm: PcmEngine::new(project_dir.clone()),
            se: SeEngine::new(project_dir.clone()),
            movie: MovieManager::new(project_dir.clone()),
            project_dir,
            solid_white,
            tables,
            stack: Vec::new(),
            unknown,
            ids,
            gfx: graphics::GfxRuntime::default(),
            ui: ui::UiRuntime::default(),
            font_cache: FontCache::new(),
            input: input::InputState::default(),
            wait: wait::VmWait::default(),
            net: net::TnmNet::default(),

            screen_w: 1280,
            screen_h: 720,
            globals: globals::GlobalState::default(),
            excall_state: ExcallCompatState::default(),
            external_forms: None,
        }
    }

    pub fn reset_for_scene_restart(&mut self) {
        self.layers.clear_all();
        self.gfx = graphics::GfxRuntime::default();
        self.ui = ui::UiRuntime::default();
        self.wait = wait::VmWait::default();
        self.stack.clear();
        self.globals = globals::GlobalState::default();
        self.excall_state = ExcallCompatState::default();
        self.input.clear_all();
    }

    /// Install or clear an external form handler.
    pub fn set_external_form_handler(&mut self, h: Option<Arc<dyn ExternalFormHandler>>) {
        self.external_forms = h;
    }

    // ------------------------------------------------------------------
    // Object button runtime
    // ------------------------------------------------------------------

    fn load_any_image_for_hit(
        images: &mut ImageManager,
        file: &str,
        patno: i64,
    ) -> Option<crate::image_manager::ImageId> {
        let pat_u32 = if patno < 0 { 0 } else { patno as u32 };
        if let Ok(id) = images.load_g00(file, pat_u32) {
            return Some(id);
        }
        if let Ok(id) = images.load_bg(file) {
            return Some(id);
        }
        None
    }

    fn hit_test_sprite_rect(x: i32, y: i32, w: u32, h: u32, mx: i32, my: i32) -> bool {
        let x2 = x.saturating_add(w as i32);
        let y2 = y.saturating_add(h as i32);
        mx >= x && mx < x2 && my >= y && my < y2
    }

    fn alpha_test_image(img: &crate::assets::RgbaImage, local_x: i32, local_y: i32) -> bool {
        if local_x < 0 || local_y < 0 {
            return false;
        }
        let lx = local_x as u32;
        let ly = local_y as u32;
        if lx >= img.width || ly >= img.height {
            return false;
        }
        let idx = ((ly * img.width + lx) * 4 + 3) as usize;
        img.rgba.get(idx).copied().unwrap_or(0) != 0
    }

    fn update_object_button_hover(&mut self) {
        let mx = self.input.mouse_x;
        let my = self.input.mouse_y;

        // We only support stage-scoped object buttons (STAGE form).
        let form_id = self.ids.form_global_stage;
        let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
            return;
        };

        let (object_lists, group_lists) = (&mut st.object_lists, &mut st.group_lists);

        // Clear per-object hit flags.
        for (_stage_idx, objs) in object_lists.iter_mut() {
            for o in objs.iter_mut() {
                if o.button.enabled {
                    o.button.hit = false;
                }
            }
        }

        // For each group, find the topmost hit object.
        for (stage_idx, groups) in group_lists.iter_mut() {
            for (group_idx, g) in groups.iter_mut().enumerate() {
                let mut best: Option<(i64, i64, usize)> = None; // (button_no, draw_order, obj_idx)

                let Some(objs) = object_lists.get_mut(stage_idx) else {
                    g.hit_button_no = -1;
                    continue;
                };

                for (obj_i, obj) in objs.iter_mut().enumerate() {
                    if !obj.used || !obj.button.enabled || obj.button.is_disabled() {
                        continue;
                    }

                    let Some(target_group) = obj.button.group_idx() else {
                        continue;
                    };
                    if target_group != group_idx {
                        continue;
                    }

                    // Visibility gate.
                    let visible = match obj.backend {
                        globals::ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            ..
                        } => self
                            .layers
                            .layer(layer_id)
                            .and_then(|l| l.sprite(sprite_id))
                            .map(|spr| spr.visible)
                            .unwrap_or(false),
                        globals::ObjectBackend::Gfx => {
                            self.gfx
                                .object_peek_disp(*stage_idx, obj_i as i64)
                                .unwrap_or(0)
                                != 0
                        }
                        _ => false,
                    };
                    if !visible {
                        continue;
                    }

                    // Bounding box + optional alpha test.
                    let mut hit = false;
                    match obj.backend {
                        globals::ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            width,
                            height,
                        } => {
                            if let Some(spr) = self
                                .layers
                                .layer(layer_id)
                                .and_then(|l| l.sprite(sprite_id))
                            {
                                hit =
                                    Self::hit_test_sprite_rect(spr.x, spr.y, width, height, mx, my);
                                if hit && obj.button.alpha_test {
                                    if let Some(img_id) = spr.image_id {
                                        if let Some(img) =
                                            self.images.get(img_id).map(|a| a.as_ref())
                                        {
                                            hit =
                                                Self::alpha_test_image(img, mx - spr.x, my - spr.y);
                                        }
                                    }
                                }
                            }
                        }
                        globals::ObjectBackend::Gfx => {
                            let (x, y) = self
                                .gfx
                                .object_peek_pos(*stage_idx, obj_i as i64)
                                .unwrap_or((0, 0));
                            let patno = self
                                .gfx
                                .object_peek_patno(*stage_idx, obj_i as i64)
                                .unwrap_or(0);
                            if let Some(file) = obj.file_name.as_deref() {
                                if let Some(img_id) =
                                    Self::load_any_image_for_hit(&mut self.images, file, patno)
                                {
                                    if let Some(img) = self.images.get(img_id).map(|a| a.as_ref()) {
                                        hit = Self::hit_test_sprite_rect(
                                            x as i32, y as i32, img.width, img.height, mx, my,
                                        );
                                        if hit && obj.button.alpha_test {
                                            hit = Self::alpha_test_image(
                                                img,
                                                mx - x as i32,
                                                my - y as i32,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }

                    if !hit {
                        continue;
                    }

                    // Draw order (conservative): use sprite order if present, else layer/order.
                    let draw_order = match obj.backend {
                        globals::ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            ..
                        } => self
                            .layers
                            .layer(layer_id)
                            .and_then(|l| l.sprite(sprite_id))
                            .map(|spr| spr.order as i64)
                            .unwrap_or(0),
                        globals::ObjectBackend::Gfx => {
                            let layer_no = self
                                .gfx
                                .object_peek_layer(*stage_idx, obj_i as i64)
                                .unwrap_or(0);
                            let order = self
                                .gfx
                                .object_peek_order(*stage_idx, obj_i as i64)
                                .unwrap_or(0);
                            layer_no.saturating_mul(1000).saturating_add(order)
                        }
                        _ => 0,
                    };

                    let btn_no = obj.button.button_no;
                    match best {
                        None => best = Some((btn_no, draw_order, obj_i)),
                        Some((_b, bo, _oi)) if draw_order > bo => {
                            best = Some((btn_no, draw_order, obj_i))
                        }
                        _ => {}
                    }
                }

                if let Some((btn_no, _ord, obj_i)) = best {
                    g.hit_button_no = btn_no;
                    if obj_i < objs.len() {
                        objs[obj_i].button.hit = true;
                    }
                } else {
                    g.hit_button_no = -1;
                }
            }
        }
    }

    fn handle_object_button_mouse_down(&mut self, b: input::VmMouseButton) {
        // Ensure hover state is up-to-date.
        self.update_object_button_hover();

        let form_id = self.ids.form_global_stage;
        let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
            return;
        };

        let (object_lists, group_lists) = (&mut st.object_lists, &mut st.group_lists);

        for (stage_idx, groups) in group_lists.iter_mut() {
            for (group_idx, g) in groups.iter_mut().enumerate() {
                let hit = g.hit_button_no;
                if hit < 0 {
                    continue;
                }

                match b {
                    input::VmMouseButton::Left => {
                        g.pushed_button_no = hit;
                        // If selection is waiting, decide immediately.
                        if g.wait_flag {
                            g.result = 1;
                            g.result_button_no = hit;
                            g.decided_button_no = hit;
                            g.wait_flag = false;
                            g.started = false;
                            self.globals.focused_stage_group = None;
                        }

                        // Mark pushed on the owning object.
                        if let Some(objs) = object_lists.get_mut(stage_idx) {
                            for obj in objs.iter_mut() {
                                if obj.button.enabled
                                    && obj.button.group_idx() == Some(group_idx)
                                    && obj.button.button_no == hit
                                {
                                    obj.button.pushed = true;
                                }
                            }
                        }
                    }
                    input::VmMouseButton::Right => {
                        if g.wait_flag && g.cancel_flag {
                            g.result = -1;
                            g.result_button_no = -1;
                            g.decided_button_no = -1;
                            g.wait_flag = false;
                            g.started = false;
                            self.globals.focused_stage_group = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_object_button_mouse_up(&mut self, b: input::VmMouseButton) {
        let form_id = self.ids.form_global_stage;
        let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
            return;
        };

        let (object_lists, group_lists) = (&mut st.object_lists, &mut st.group_lists);

        if matches!(b, input::VmMouseButton::Left) {
            // Conservative: clear pushed flag unless a script keeps it.
            for (_stage_idx, groups) in group_lists.iter_mut() {
                for g in groups.iter_mut() {
                    g.pushed_button_no = -1;
                }
            }
            for (_stage_idx, objs) in object_lists.iter_mut() {
                for o in objs.iter_mut() {
                    if o.button.enabled && !o.button.push_keep {
                        o.button.pushed = false;
                    }
                }
            }
        }
    }
    // ------------------------------------------------------------------
    // Input bridge (platform event -> VM state)
    // ------------------------------------------------------------------

    pub fn on_key_down(&mut self, k: input::VmKey) {
        if self.handle_syscom_menu_key(k) {
            return;
        }
        self.input.on_key_down(k);

        // EditBox runtime: map common keys and focus changes.
        self.handle_editbox_key(k);

        let handled_mwnd_selection = self.handle_mwnd_selection_key(k);

        // Stage group selection runtime: map Enter/Escape to a decision.
        if !handled_mwnd_selection {
            if let Some((form_id, stage_idx, group_idx)) = self.globals.focused_stage_group {
                if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
                    if let Some(list) = st.group_lists.get_mut(&stage_idx) {
                        if let Some(g) = list.get_mut(group_idx) {
                            match k {
                                input::VmKey::Enter => {
                                    g.result = 1;
                                    g.result_button_no = 0;
                                    g.decided_button_no = 0;
                                    g.wait_flag = false;
                                    g.started = false;
                                    self.globals.focused_stage_group = None;
                                }
                                input::VmKey::Escape => {
                                    g.result = -1;
                                    g.result_button_no = -1;
                                    g.decided_button_no = -1;
                                    g.wait_flag = false;
                                    g.started = false;
                                    self.globals.focused_stage_group = None;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        self.advance_message_wait(true);
        let wipe_skipped = self.wait.notify_key();
        while let Some(info) = self.wait.take_movie_skip() {
            let (globals, movie_mgr, layers) =
                (&mut self.globals, &mut self.movie, &mut self.layers);
            if let Some(st) = globals.stage_forms.get_mut(&info.stage_form_id) {
                if let Some(list) = st.object_lists.get_mut(&info.stage_idx) {
                    if info.obj_idx < list.len() {
                        // key skip triggers C_elm_object::init_type(true).
                        let (audio_id, backend) = {
                            let obj = &mut list[info.obj_idx];
                            let audio_id = obj.movie.audio_id.take();
                            let backend = obj.backend.clone();
                            obj.init_type_like();
                            (audio_id, backend)
                        };
                        if let Some(id) = audio_id {
                            movie_mgr.stop_audio(id);
                        }
                        if let globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id,
                            ..
                        } = backend
                        {
                            if let Some(layer) = layers.layer_mut(layer_id) {
                                if let Some(sprite) = layer.sprite_mut(sprite_id) {
                                    sprite.visible = false;
                                    sprite.image_id = None;
                                }
                            }
                        }
                    }
                }
            }
        }
        if wipe_skipped {
            self.globals.finish_wipe();
        }
    }

    pub fn on_key_up(&mut self, k: input::VmKey) {
        self.input.on_key_up(k);
    }

    pub fn on_text_input(&mut self, text: &str) {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        let Some(list) = self.globals.editbox_lists.get_mut(&form_id) else {
            return;
        };
        let Some(eb) = list.boxes.get_mut(idx) else {
            return;
        };
        if !eb.alive {
            return;
        }
        if !text.is_empty() {
            eb.text.push_str(text);
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32) {
        self.input.on_mouse_move(x, y);
        self.update_object_button_hover();
    }

    pub fn on_mouse_down(&mut self, b: input::VmMouseButton) {
        if self.handle_syscom_menu_click() {
            return;
        }
        let handled_mwnd_selection = self.handle_mwnd_selection_click(b);
        self.input.on_mouse_down(b);
        if !handled_mwnd_selection {
            self.handle_object_button_mouse_down(b);
        }
        self.advance_message_wait(true);
        let wipe_skipped = self.wait.notify_key();
        while let Some(info) = self.wait.take_movie_skip() {
            let (globals, movie_mgr, layers) =
                (&mut self.globals, &mut self.movie, &mut self.layers);
            if let Some(st) = globals.stage_forms.get_mut(&info.stage_form_id) {
                if let Some(list) = st.object_lists.get_mut(&info.stage_idx) {
                    if info.obj_idx < list.len() {
                        // key skip triggers C_elm_object::init_type(true).
                        let (audio_id, backend) = {
                            let obj = &mut list[info.obj_idx];
                            let audio_id = obj.movie.audio_id.take();
                            let backend = obj.backend.clone();
                            obj.init_type_like();
                            (audio_id, backend)
                        };
                        if let Some(id) = audio_id {
                            movie_mgr.stop_audio(id);
                        }
                        if let globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id,
                            ..
                        } = backend
                        {
                            if let Some(layer) = layers.layer_mut(layer_id) {
                                if let Some(sprite) = layer.sprite_mut(sprite_id) {
                                    sprite.visible = false;
                                    sprite.image_id = None;
                                }
                            }
                        }
                    }
                }
            }
        }
        if wipe_skipped {
            self.globals.finish_wipe();
        }
    }

    pub fn on_mouse_up(&mut self, b: input::VmMouseButton) {
        self.input.on_mouse_up(b);
        self.handle_object_button_mouse_up(b);
    }

    pub fn on_mouse_wheel(&mut self, delta_y: i32) {
        self.input.on_mouse_wheel(delta_y);
        self.advance_message_wait(self.should_wheel_advance_message());
        let wipe_skipped = self.wait.notify_key();
        while let Some(info) = self.wait.take_movie_skip() {
            let (globals, movie_mgr, layers) =
                (&mut self.globals, &mut self.movie, &mut self.layers);
            if let Some(st) = globals.stage_forms.get_mut(&info.stage_form_id) {
                if let Some(list) = st.object_lists.get_mut(&info.stage_idx) {
                    if info.obj_idx < list.len() {
                        // key skip triggers C_elm_object::init_type(true).
                        let (audio_id, backend) = {
                            let obj = &mut list[info.obj_idx];
                            let audio_id = obj.movie.audio_id.take();
                            let backend = obj.backend.clone();
                            obj.init_type_like();
                            (audio_id, backend)
                        };
                        if let Some(id) = audio_id {
                            movie_mgr.stop_audio(id);
                        }
                        if let globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id,
                            ..
                        } = backend
                        {
                            if let Some(layer) = layers.layer_mut(layer_id) {
                                if let Some(sprite) = layer.sprite_mut(sprite_id) {
                                    sprite.visible = false;
                                    sprite.image_id = None;
                                }
                            }
                        }
                    }
                }
            }
        }
        if wipe_skipped {
            self.globals.finish_wipe();
        }
    }

    fn handle_editbox_key(&mut self, k: input::VmKey) {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        let Some(list) = self.globals.editbox_lists.get_mut(&form_id) else {
            return;
        };
        let Some(eb) = list.boxes.get_mut(idx) else {
            return;
        };
        if !eb.alive {
            return;
        }

        match k {
            input::VmKey::Enter => {
                eb.decided = true;
            }
            input::VmKey::Escape => {
                eb.canceled = true;
            }
            input::VmKey::Backspace => {
                eb.text.pop();
            }
            input::VmKey::Tab => {
                let next = if list.boxes.is_empty() {
                    None
                } else {
                    let len = list.boxes.len() as i32;
                    let mut cur = idx as i32;
                    let mut found = None;
                    for _ in 0..len {
                        cur = (cur + 1).rem_euclid(len);
                        if let Some(b) = list.boxes.get(cur as usize) {
                            if b.alive {
                                found = Some(cur as usize);
                                break;
                            }
                        }
                    }
                    found
                };
                if let Some(n) = next {
                    self.globals.focused_editbox = Some((form_id, n));
                }
            }
            _ => {}
        }
    }

    pub fn wait_poll(&mut self) -> bool {
        let (wait, stack, bgm, se, pcm, globals) = (
            &mut self.wait,
            &mut self.stack,
            &mut self.bgm,
            &mut self.se,
            &mut self.pcm,
            &mut self.globals,
        );
        wait.poll(stack, bgm, se, pcm, globals)
    }

    pub fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    pub fn pop(&mut self) -> Option<Value> {
        self.stack.pop()
    }

    pub fn set_screen_size(&mut self, w: u32, h: u32) {
        self.screen_w = w;
        self.screen_h = h;
        self.ui.sync_layout(&mut self.layers, w, h);
    }

    pub fn tick_frame(&mut self) {
        self.ui.tick(
            &mut self.layers,
            &mut self.images,
            &self.project_dir,
            self.screen_w,
            self.screen_h,
            &self.globals.script,
            &self.globals.syscom,
        );
        // Apply syscom flags that should skip visual transitions immediately.
        self.apply_syscom_skip_flags();
        // Sync message length for auto-mode timing.
        self.globals.script.auto_mode_moji_cnt = self
            .ui
            .current_message
            .as_deref()
            .unwrap_or("")
            .chars()
            .count() as i64;
        if self
            .ui
            .auto_advance_due(&self.globals.script, &self.globals.syscom)
        {
            self.advance_message_wait(true);
        }
        // If scripts request message-window hide, enforce it after UI tick.
        if self.globals.script.mwnd_disp_off_flag {
            self.ui.show_message_bg(false);
        }
        self.sync_syscom_menu_ui();
        self.sync_mwnd_selection_ui();
        self.globals.tick_frame();
        self.sync_movie_objects();
        self.apply_object_event_animations();
        self.apply_object_disp_override();
    }

    fn apply_syscom_skip_flags(&mut self) {
        const GET_NO_WIPE_ANIME_ONOFF: i32 = 286;
        const GET_SKIP_WIPE_ANIME_ONOFF: i32 = 288;
        let cfg = &self.globals.syscom.config_int;
        let no_wipe = cfg.get(&GET_NO_WIPE_ANIME_ONOFF).copied().unwrap_or(0) != 0;
        let skip_wipe = cfg.get(&GET_SKIP_WIPE_ANIME_ONOFF).copied().unwrap_or(0) != 0;
        if (no_wipe || skip_wipe) && self.globals.wipe.is_some() {
            self.globals.finish_wipe();
        }
    }

    fn apply_object_event_animations(&mut self) {
        for st in self.globals.stage_forms.values_mut() {
            for (stage_idx, objs) in st.object_lists.iter_mut() {
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    if obj.extra_events.is_empty() && obj.rep_int_event_lists.is_empty() {
                        continue;
                    }

                    let mut x: Option<i64> = None;
                    let mut y: Option<i64> = None;
                    let mut alpha: Option<i64> = None;
                    let mut patno: Option<i64> = None;
                    let mut order: Option<i64> = None;
                    let mut layer_no: Option<i64> = None;
                    let mut z: Option<i64> = None;
                    let mut center_x: Option<i64> = None;
                    let mut center_y: Option<i64> = None;
                    let mut scale_x: Option<i64> = None;
                    let mut scale_y: Option<i64> = None;
                    let mut rotate_z: Option<i64> = None;
                    let mut clip_left: Option<i64> = None;
                    let mut clip_top: Option<i64> = None;
                    let mut clip_right: Option<i64> = None;
                    let mut clip_bottom: Option<i64> = None;
                    let mut src_clip_left: Option<i64> = None;
                    let mut src_clip_top: Option<i64> = None;
                    let mut src_clip_right: Option<i64> = None;
                    let mut src_clip_bottom: Option<i64> = None;
                    let mut tr: Option<i64> = None;
                    let mut mono: Option<i64> = None;
                    let mut reverse: Option<i64> = None;
                    let mut bright: Option<i64> = None;
                    let mut dark: Option<i64> = None;
                    let mut color_rate: Option<i64> = None;
                    let mut color_add_r: Option<i64> = None;
                    let mut color_add_g: Option<i64> = None;
                    let mut color_add_b: Option<i64> = None;
                    let mut color_r: Option<i64> = None;
                    let mut color_g: Option<i64> = None;
                    let mut color_b: Option<i64> = None;

                    let ops: Vec<i32> = obj.extra_events.keys().copied().collect();
                    for op in ops {
                        let t = obj.event_target(op);
                        let v = obj
                            .extra_events
                            .get(&op)
                            .map(|ev| ev.get_total_value() as i64)
                            .unwrap_or(0);
                        match t {
                            globals::ObjectEventTarget::X => x = Some(v),
                            globals::ObjectEventTarget::Y => y = Some(v),
                            globals::ObjectEventTarget::Alpha => alpha = Some(v),
                            globals::ObjectEventTarget::Patno => patno = Some(v),
                            globals::ObjectEventTarget::Order => order = Some(v),
                            globals::ObjectEventTarget::Layer => layer_no = Some(v),
                            globals::ObjectEventTarget::Z => z = Some(v),
                            globals::ObjectEventTarget::CenterX => center_x = Some(v),
                            globals::ObjectEventTarget::CenterY => center_y = Some(v),
                            globals::ObjectEventTarget::ScaleX => scale_x = Some(v),
                            globals::ObjectEventTarget::ScaleY => scale_y = Some(v),
                            globals::ObjectEventTarget::RotateZ => rotate_z = Some(v),
                            globals::ObjectEventTarget::ClipLeft => clip_left = Some(v),
                            globals::ObjectEventTarget::ClipTop => clip_top = Some(v),
                            globals::ObjectEventTarget::ClipRight => clip_right = Some(v),
                            globals::ObjectEventTarget::ClipBottom => clip_bottom = Some(v),
                            globals::ObjectEventTarget::SrcClipLeft => src_clip_left = Some(v),
                            globals::ObjectEventTarget::SrcClipTop => src_clip_top = Some(v),
                            globals::ObjectEventTarget::SrcClipRight => src_clip_right = Some(v),
                            globals::ObjectEventTarget::SrcClipBottom => src_clip_bottom = Some(v),
                            globals::ObjectEventTarget::Tr => tr = Some(v),
                            globals::ObjectEventTarget::Mono => mono = Some(v),
                            globals::ObjectEventTarget::Reverse => reverse = Some(v),
                            globals::ObjectEventTarget::Bright => bright = Some(v),
                            globals::ObjectEventTarget::Dark => dark = Some(v),
                            globals::ObjectEventTarget::ColorRate => color_rate = Some(v),
                            globals::ObjectEventTarget::ColorAddR => color_add_r = Some(v),
                            globals::ObjectEventTarget::ColorAddG => color_add_g = Some(v),
                            globals::ObjectEventTarget::ColorAddB => color_add_b = Some(v),
                            globals::ObjectEventTarget::ColorR => color_r = Some(v),
                            globals::ObjectEventTarget::ColorG => color_g = Some(v),
                            globals::ObjectEventTarget::ColorB => color_b = Some(v),
                            globals::ObjectEventTarget::Unknown => {}
                        }
                    }

                    let list_ops: Vec<i32> = obj.rep_int_event_lists.keys().copied().collect();
                    for op in list_ops {
                        let t = obj.event_target(op);
                        let v = obj
                            .rep_int_event_lists
                            .get(&op)
                            .and_then(|list| list.get(0))
                            .map(|ev| ev.get_total_value() as i64)
                            .unwrap_or(0);
                        match t {
                            globals::ObjectEventTarget::X => x = Some(v),
                            globals::ObjectEventTarget::Y => y = Some(v),
                            globals::ObjectEventTarget::Alpha => alpha = Some(v),
                            globals::ObjectEventTarget::Patno => patno = Some(v),
                            globals::ObjectEventTarget::Order => order = Some(v),
                            globals::ObjectEventTarget::Layer => layer_no = Some(v),
                            globals::ObjectEventTarget::Z => z = Some(v),
                            globals::ObjectEventTarget::CenterX => center_x = Some(v),
                            globals::ObjectEventTarget::CenterY => center_y = Some(v),
                            globals::ObjectEventTarget::ScaleX => scale_x = Some(v),
                            globals::ObjectEventTarget::ScaleY => scale_y = Some(v),
                            globals::ObjectEventTarget::RotateZ => rotate_z = Some(v),
                            globals::ObjectEventTarget::ClipLeft => clip_left = Some(v),
                            globals::ObjectEventTarget::ClipTop => clip_top = Some(v),
                            globals::ObjectEventTarget::ClipRight => clip_right = Some(v),
                            globals::ObjectEventTarget::ClipBottom => clip_bottom = Some(v),
                            globals::ObjectEventTarget::SrcClipLeft => src_clip_left = Some(v),
                            globals::ObjectEventTarget::SrcClipTop => src_clip_top = Some(v),
                            globals::ObjectEventTarget::SrcClipRight => src_clip_right = Some(v),
                            globals::ObjectEventTarget::SrcClipBottom => src_clip_bottom = Some(v),
                            globals::ObjectEventTarget::Tr => tr = Some(v),
                            globals::ObjectEventTarget::Mono => mono = Some(v),
                            globals::ObjectEventTarget::Reverse => reverse = Some(v),
                            globals::ObjectEventTarget::Bright => bright = Some(v),
                            globals::ObjectEventTarget::Dark => dark = Some(v),
                            globals::ObjectEventTarget::ColorRate => color_rate = Some(v),
                            globals::ObjectEventTarget::ColorAddR => color_add_r = Some(v),
                            globals::ObjectEventTarget::ColorAddG => color_add_g = Some(v),
                            globals::ObjectEventTarget::ColorAddB => color_add_b = Some(v),
                            globals::ObjectEventTarget::ColorR => color_r = Some(v),
                            globals::ObjectEventTarget::ColorG => color_g = Some(v),
                            globals::ObjectEventTarget::ColorB => color_b = Some(v),
                            globals::ObjectEventTarget::Unknown => {}
                        }
                    }

                    if x.is_none()
                        && y.is_none()
                        && alpha.is_none()
                        && patno.is_none()
                        && order.is_none()
                        && layer_no.is_none()
                        && z.is_none()
                        && center_x.is_none()
                        && center_y.is_none()
                        && scale_x.is_none()
                        && scale_y.is_none()
                        && rotate_z.is_none()
                        && clip_left.is_none()
                        && clip_top.is_none()
                        && clip_right.is_none()
                        && clip_bottom.is_none()
                        && src_clip_left.is_none()
                        && src_clip_top.is_none()
                        && src_clip_right.is_none()
                        && src_clip_bottom.is_none()
                        && tr.is_none()
                        && mono.is_none()
                        && reverse.is_none()
                        && bright.is_none()
                        && dark.is_none()
                        && color_rate.is_none()
                        && color_add_r.is_none()
                        && color_add_g.is_none()
                        && color_add_b.is_none()
                        && color_r.is_none()
                        && color_g.is_none()
                        && color_b.is_none()
                    {
                        continue;
                    }

                    let stage_i64 = *stage_idx;
                    let obj_i64 = obj_idx as i64;

                    match &obj.backend {
                        globals::ObjectBackend::Gfx => {
                            if let Some(ax) = x {
                                let _ = self.gfx.object_set_x(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    ax,
                                );
                            }
                            if let Some(ay) = y {
                                let _ = self.gfx.object_set_y(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    ay,
                                );
                            }
                            if let Some(a) = alpha {
                                let _ = self.gfx.object_set_alpha(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    a,
                                );
                            }
                            if let Some(p) = patno {
                                let _ = self.gfx.object_set_pat_no(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    p,
                                );
                            }
                            if let Some(o) = order {
                                let _ = self.gfx.object_set_order(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    o,
                                );
                            }
                            if let Some(l) = layer_no {
                                let _ = self.gfx.object_set_layer(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    l,
                                );
                            }
                            if let Some(zv) = z {
                                let _ = self.gfx.object_set_z(stage_i64, obj_i64, zv);
                            }
                            if let (Some(cx), Some(cy)) = (center_x, center_y) {
                                let _ = self.gfx.object_set_center(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    cx,
                                    cy,
                                );
                            }
                            if let (Some(sx), Some(sy)) = (scale_x, scale_y) {
                                let _ = self.gfx.object_set_scale(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    sx,
                                    sy,
                                );
                            }
                            if let Some(rz) = rotate_z {
                                let _ = self.gfx.object_set_rotate(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    rz,
                                );
                            }
                            if clip_left.is_some()
                                || clip_top.is_some()
                                || clip_right.is_some()
                                || clip_bottom.is_some()
                            {
                                let use_flag = if self.ids.obj_clip_use != 0 {
                                    obj.extra_int_props
                                        .get(&self.ids.obj_clip_use)
                                        .copied()
                                        .unwrap_or(0)
                                } else {
                                    0
                                };
                                let left = clip_left
                                    .or_else(|| {
                                        if self.ids.obj_clip_left != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_clip_left)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let top = clip_top
                                    .or_else(|| {
                                        if self.ids.obj_clip_top != 0 {
                                            obj.extra_int_props.get(&self.ids.obj_clip_top).copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let right = clip_right
                                    .or_else(|| {
                                        if self.ids.obj_clip_right != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_clip_right)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let bottom = clip_bottom
                                    .or_else(|| {
                                        if self.ids.obj_clip_bottom != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_clip_bottom)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let _ = self.gfx.object_set_clip(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    use_flag,
                                    left,
                                    top,
                                    right,
                                    bottom,
                                );
                            }
                            if src_clip_left.is_some()
                                || src_clip_top.is_some()
                                || src_clip_right.is_some()
                                || src_clip_bottom.is_some()
                            {
                                let use_flag = if self.ids.obj_src_clip_use != 0 {
                                    obj.extra_int_props
                                        .get(&self.ids.obj_src_clip_use)
                                        .copied()
                                        .unwrap_or(0)
                                } else {
                                    0
                                };
                                let left = src_clip_left
                                    .or_else(|| {
                                        if self.ids.obj_src_clip_left != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_src_clip_left)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let top = src_clip_top
                                    .or_else(|| {
                                        if self.ids.obj_src_clip_top != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_src_clip_top)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let right = src_clip_right
                                    .or_else(|| {
                                        if self.ids.obj_src_clip_right != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_src_clip_right)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let bottom = src_clip_bottom
                                    .or_else(|| {
                                        if self.ids.obj_src_clip_bottom != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_src_clip_bottom)
                                                .copied()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                let _ = self.gfx.object_set_src_clip(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    use_flag,
                                    left,
                                    top,
                                    right,
                                    bottom,
                                );
                            }
                            if let Some(v) = tr {
                                let _ = self.gfx.object_set_tr(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    v,
                                );
                            }
                            if let Some(v) = mono {
                                let _ = self.gfx.object_set_mono(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    v,
                                );
                            }
                            if let Some(v) = reverse {
                                let _ = self.gfx.object_set_reverse(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    v,
                                );
                            }
                            if let Some(v) = bright {
                                let _ = self.gfx.object_set_bright(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    v,
                                );
                            }
                            if let Some(v) = dark {
                                let _ = self.gfx.object_set_dark(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    v,
                                );
                            }
                            if let Some(v) = color_rate {
                                let _ = self.gfx.object_set_color_rate(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    v,
                                );
                            }
                            if color_add_r.is_some()
                                || color_add_g.is_some()
                                || color_add_b.is_some()
                            {
                                let r = color_add_r.unwrap_or_else(|| {
                                    if self.ids.obj_color_add_r != 0 {
                                        *obj.extra_int_props
                                            .get(&self.ids.obj_color_add_r)
                                            .unwrap_or(&0)
                                    } else {
                                        0
                                    }
                                });
                                let g = color_add_g.unwrap_or_else(|| {
                                    if self.ids.obj_color_add_g != 0 {
                                        *obj.extra_int_props
                                            .get(&self.ids.obj_color_add_g)
                                            .unwrap_or(&0)
                                    } else {
                                        0
                                    }
                                });
                                let b = color_add_b.unwrap_or_else(|| {
                                    if self.ids.obj_color_add_b != 0 {
                                        *obj.extra_int_props
                                            .get(&self.ids.obj_color_add_b)
                                            .unwrap_or(&0)
                                    } else {
                                        0
                                    }
                                });
                                let _ = self.gfx.object_set_color_add(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    r,
                                    g,
                                    b,
                                );
                            }
                            if color_r.is_some() || color_g.is_some() || color_b.is_some() {
                                let r = color_r.unwrap_or_else(|| {
                                    if self.ids.obj_color_r != 0 {
                                        *obj.extra_int_props
                                            .get(&self.ids.obj_color_r)
                                            .unwrap_or(&0)
                                    } else {
                                        0
                                    }
                                });
                                let g = color_g.unwrap_or_else(|| {
                                    if self.ids.obj_color_g != 0 {
                                        *obj.extra_int_props
                                            .get(&self.ids.obj_color_g)
                                            .unwrap_or(&0)
                                    } else {
                                        0
                                    }
                                });
                                let b = color_b.unwrap_or_else(|| {
                                    if self.ids.obj_color_b != 0 {
                                        *obj.extra_int_props
                                            .get(&self.ids.obj_color_b)
                                            .unwrap_or(&0)
                                    } else {
                                        0
                                    }
                                });
                                let _ = self.gfx.object_set_color(
                                    &mut self.images,
                                    &mut self.layers,
                                    stage_i64,
                                    obj_i64,
                                    r,
                                    g,
                                    b,
                                );
                            }
                        }
                        globals::ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            ..
                        }
                        | globals::ObjectBackend::String {
                            layer_id,
                            sprite_id,
                            ..
                        }
                        | globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id,
                            ..
                        } => {
                            if let Some(layer) = self.layers.layer_mut(*layer_id) {
                                if let Some(sprite) = layer.sprite_mut(*sprite_id) {
                                    if let Some(ax) = x {
                                        sprite.x = ax as i32;
                                    }
                                    if let Some(ay) = y {
                                        sprite.y = ay as i32;
                                    }
                                    if let Some(a) = alpha {
                                        sprite.alpha = a.clamp(0, 255) as u8;
                                    }
                                    if let (Some(cx), Some(cy)) = (center_x, center_y) {
                                        sprite.pivot_x = cx as f32;
                                        sprite.pivot_y = cy as f32;
                                    }
                                    if let (Some(sx), Some(sy)) = (scale_x, scale_y) {
                                        sprite.scale_x = sx as f32 / 1000.0;
                                        sprite.scale_y = sy as f32 / 1000.0;
                                    }
                                    if let Some(rz) = rotate_z {
                                        sprite.rotate = rz as f32 * std::f32::consts::PI / 1800.0;
                                    }
                                    if clip_left.is_some()
                                        || clip_top.is_some()
                                        || clip_right.is_some()
                                        || clip_bottom.is_some()
                                    {
                                        let use_flag = if self.ids.obj_clip_use != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_clip_use)
                                                .copied()
                                                .unwrap_or(0)
                                        } else {
                                            0
                                        };
                                        if use_flag != 0 {
                                            let left = clip_left
                                                .or_else(|| {
                                                    if self.ids.obj_clip_left != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_clip_left)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            let top = clip_top
                                                .or_else(|| {
                                                    if self.ids.obj_clip_top != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_clip_top)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            let right = clip_right
                                                .or_else(|| {
                                                    if self.ids.obj_clip_right != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_clip_right)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            let bottom = clip_bottom
                                                .or_else(|| {
                                                    if self.ids.obj_clip_bottom != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_clip_bottom)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            sprite.dst_clip = Some(crate::layer::ClipRect {
                                                left: left as i32,
                                                top: top as i32,
                                                right: right as i32,
                                                bottom: bottom as i32,
                                            });
                                        } else {
                                            sprite.dst_clip = None;
                                        }
                                    }
                                    if src_clip_left.is_some()
                                        || src_clip_top.is_some()
                                        || src_clip_right.is_some()
                                        || src_clip_bottom.is_some()
                                    {
                                        let use_flag = if self.ids.obj_src_clip_use != 0 {
                                            obj.extra_int_props
                                                .get(&self.ids.obj_src_clip_use)
                                                .copied()
                                                .unwrap_or(0)
                                        } else {
                                            0
                                        };
                                        if use_flag != 0 {
                                            let left = src_clip_left
                                                .or_else(|| {
                                                    if self.ids.obj_src_clip_left != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_src_clip_left)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            let top = src_clip_top
                                                .or_else(|| {
                                                    if self.ids.obj_src_clip_top != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_src_clip_top)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            let right = src_clip_right
                                                .or_else(|| {
                                                    if self.ids.obj_src_clip_right != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_src_clip_right)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            let bottom = src_clip_bottom
                                                .or_else(|| {
                                                    if self.ids.obj_src_clip_bottom != 0 {
                                                        obj.extra_int_props
                                                            .get(&self.ids.obj_src_clip_bottom)
                                                            .copied()
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            sprite.src_clip = Some(crate::layer::ClipRect {
                                                left: left as i32,
                                                top: top as i32,
                                                right: right as i32,
                                                bottom: bottom as i32,
                                            });
                                        } else {
                                            sprite.src_clip = None;
                                        }
                                    }
                                    if let Some(v) = tr {
                                        sprite.tr = v.clamp(0, 255) as u8;
                                    }
                                    if let Some(v) = mono {
                                        sprite.mono = v.clamp(0, 255) as u8;
                                    }
                                    if let Some(v) = reverse {
                                        sprite.reverse = v.clamp(0, 255) as u8;
                                    }
                                    if let Some(v) = bright {
                                        sprite.bright = v.clamp(0, 255) as u8;
                                    }
                                    if let Some(v) = dark {
                                        sprite.dark = v.clamp(0, 255) as u8;
                                    }
                                    if let Some(v) = color_rate {
                                        sprite.color_rate = v.clamp(0, 255) as u8;
                                    }
                                    if color_add_r.is_some()
                                        || color_add_g.is_some()
                                        || color_add_b.is_some()
                                    {
                                        sprite.color_add_r =
                                            color_add_r.unwrap_or(0).clamp(0, 255) as u8;
                                        sprite.color_add_g =
                                            color_add_g.unwrap_or(0).clamp(0, 255) as u8;
                                        sprite.color_add_b =
                                            color_add_b.unwrap_or(0).clamp(0, 255) as u8;
                                    }
                                    if color_r.is_some() || color_g.is_some() || color_b.is_some() {
                                        sprite.color_r = color_r.unwrap_or(0).clamp(0, 255) as u8;
                                        sprite.color_g = color_g.unwrap_or(0).clamp(0, 255) as u8;
                                        sprite.color_b = color_b.unwrap_or(0).clamp(0, 255) as u8;
                                    }
                                }
                            }
                        }
                        globals::ObjectBackend::Number {
                            layer_id,
                            sprite_ids,
                        } => {
                            if let Some(a) = alpha {
                                if let Some(layer) = self.layers.layer_mut(*layer_id) {
                                    for &sid in sprite_ids {
                                        if let Some(sprite) = layer.sprite_mut(sid) {
                                            sprite.alpha = a.clamp(0, 255) as u8;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn apply_gan_effects(&mut self, sprites: &mut Vec<RenderSprite>) {
        let mut index: HashMap<(Option<LayerId>, Option<SpriteId>), usize> = HashMap::new();
        for (i, s) in sprites.iter().enumerate() {
            index.insert((s.layer_id, s.sprite_id), i);
        }

        for st in self.globals.stage_forms.values() {
            for (stage_idx, objs) in st.object_lists.iter() {
                for (obj_idx, obj) in objs.iter().enumerate() {
                    let Some(pat) = obj.gan.current_pat() else {
                        continue;
                    };
                    if pat.pat_no == 0 && pat.x == 0 && pat.y == 0 && pat.tr == 255 {
                        continue;
                    }

                    let key: Option<(LayerId, SpriteId)> = match &obj.backend {
                        globals::ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            ..
                        }
                        | globals::ObjectBackend::String {
                            layer_id,
                            sprite_id,
                            ..
                        }
                        | globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id,
                            ..
                        } => Some((*layer_id, *sprite_id)),
                        globals::ObjectBackend::Gfx => {
                            self.gfx.object_sprite_binding(*stage_idx, obj_idx as i64)
                        }
                        _ => None,
                    };
                    let Some((layer_id, sprite_id)) = key else {
                        continue;
                    };
                    let Some(&idx) = index.get(&(Some(layer_id), Some(sprite_id))) else {
                        continue;
                    };

                    let sprite = &mut sprites[idx].sprite;
                    if pat.x != 0 {
                        sprite.x = sprite.x.saturating_add(pat.x);
                    }
                    if pat.y != 0 {
                        sprite.y = sprite.y.saturating_add(pat.y);
                    }
                    if pat.tr != 255 {
                        let tr = (sprite.tr as i64 * pat.tr as i64 / 255).clamp(0, 255) as u8;
                        sprite.tr = tr;
                    }

                    if pat.pat_no != 0 {
                        if let Some(file) = self.gfx.object_peek_file(*stage_idx, obj_idx as i64) {
                            let base_pat = self
                                .gfx
                                .object_peek_patno(*stage_idx, obj_idx as i64)
                                .unwrap_or(0);
                            let pat_no = (base_pat + pat.pat_no as i64).max(0) as u32;
                            if let Ok(id) = self.images.load_g00(&file, pat_no) {
                                sprite.image_id = Some(id);
                            }
                        }
                    }
                }
            }
        }
    }

    fn apply_object_masks(&mut self) {
        let Some(mask_info) = self.build_mask_info() else {
            return;
        };
        if mask_info.is_empty() {
            return;
        }

        let mut resolved_masks = HashMap::new();
        for (mask_name, _, _) in mask_info.iter().flatten() {
            if resolved_masks.contains_key(mask_name) {
                continue;
            }
            if let Some(id) = self.resolve_mask_image(mask_name) {
                resolved_masks.insert(mask_name.clone(), id);
            }
        }

        for st in self.globals.stage_forms.values_mut() {
            for (stage_idx, objs) in st.object_lists.iter_mut() {
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    let mask_no = if self.ids.obj_mask_no != 0 {
                        *obj.extra_int_props
                            .get(&self.ids.obj_mask_no)
                            .unwrap_or(&-1)
                    } else {
                        -1
                    };
                    if mask_no < 0 {
                        continue;
                    }
                    let mask_idx = mask_no as usize;
                    if mask_idx >= mask_info.len() {
                        continue;
                    }
                    let Some((ref mask_name, mask_x, mask_y)) = mask_info[mask_idx] else {
                        continue;
                    };

                    let mask_image_id = match resolved_masks.get(mask_name).copied() {
                        Some(id) => id,
                        None => continue,
                    };

                    let targets: Vec<(LayerId, SpriteId)> = match &obj.backend {
                        globals::ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            ..
                        }
                        | globals::ObjectBackend::String {
                            layer_id,
                            sprite_id,
                            ..
                        }
                        | globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id,
                            ..
                        } => {
                            vec![(*layer_id, *sprite_id)]
                        }
                        globals::ObjectBackend::Number {
                            layer_id,
                            sprite_ids,
                        } => sprite_ids.iter().map(|sid| (*layer_id, *sid)).collect(),
                        globals::ObjectBackend::Gfx => self
                            .gfx
                            .object_sprite_binding(*stage_idx, obj_idx as i64)
                            .into_iter()
                            .collect(),
                        _ => Vec::new(),
                    };

                    for (layer_id, sprite_id) in targets {
                        let Some(sprite) = self
                            .layers
                            .layer_mut(layer_id)
                            .and_then(|l| l.sprite_mut(sprite_id))
                        else {
                            continue;
                        };
                        let Some(base_id) = sprite.image_id else {
                            continue;
                        };

                        let (base_img, base_ver) = match self.images.get_entry(base_id) {
                            Some(v) => v,
                            None => continue,
                        };
                        let (mask_img, mask_ver) = match self.images.get_entry(mask_image_id) {
                            Some(v) => v,
                            None => continue,
                        };

                        let key = (layer_id, sprite_id);
                        let cached = obj.mask_cache.get(&key);
                        if let Some(cache) = cached {
                            if cache.base_image_id == base_id
                                && cache.base_version == base_ver
                                && cache.mask_image_id == mask_image_id
                                && cache.mask_version == mask_ver
                                && cache.mask_x == mask_x
                                && cache.mask_y == mask_y
                            {
                                sprite.image_id = Some(cache.masked_image_id);
                                continue;
                            }
                        }

                        let masked = apply_mask_image(base_img, mask_img, mask_x, mask_y);
                        let masked_id = self.images.insert_image(masked);

                        obj.mask_cache.insert(
                            key,
                            globals::MaskedSpriteCache {
                                base_image_id: base_id,
                                base_version: base_ver,
                                mask_image_id,
                                mask_version: mask_ver,
                                mask_x,
                                mask_y,
                                masked_image_id: masked_id,
                            },
                        );
                        sprite.image_id = Some(masked_id);
                    }
                }
            }
        }
    }

    fn active_mask_list(&self) -> Option<&globals::MaskListState> {
        if self.ids.form_global_mask != 0 {
            if let Some(ml) = self.globals.mask_lists.get(&self.ids.form_global_mask) {
                return Some(ml);
            }
        }
        let fid = self.globals.guessed_mask_form_id?;
        self.globals.mask_lists.get(&fid)
    }

    fn build_mask_info(&self) -> Option<Vec<Option<(String, i32, i32)>>> {
        let ml = self.active_mask_list()?;
        let mut out = Vec::with_capacity(ml.masks.len());
        for m in &ml.masks {
            let Some(name) = m.name.as_ref() else {
                out.push(None);
                continue;
            };
            if name.is_empty() {
                out.push(None);
                continue;
            }
            let x = m.x_event.get_total_value();
            let y = m.y_event.get_total_value();
            out.push(Some((name.clone(), x, y)));
        }
        Some(out)
    }

    fn resolve_mask_image(&mut self, name: &str) -> Option<ImageId> {
        if name.is_empty() {
            return None;
        }
        if let Some(path) = resolve_mask_path(&self.project_dir, name) {
            if let Ok(id) = self.images.load_file(&path, 0) {
                return Some(id);
            }
        }
        if let Ok(id) = self.images.load_g00(name, 0) {
            return Some(id);
        }
        if let Ok(id) = self.images.load_bg(name) {
            return Some(id);
        }
        None
    }

    fn apply_object_disp_override(&mut self) {
        const GET_OBJECT_DISP_ONOFF: i32 = 278;
        let disp_on = self
            .globals
            .syscom
            .config_int
            .get(&GET_OBJECT_DISP_ONOFF)
            .copied()
            .unwrap_or(1)
            != 0;
        if disp_on {
            return;
        }

        let ui_layer = self.ui.ui_layer;
        for (stage_idx, list) in self
            .globals
            .stage_forms
            .values()
            .flat_map(|st| st.object_lists.iter())
        {
            for (obj_idx, obj) in list.iter().enumerate() {
                match &obj.backend {
                    globals::ObjectBackend::Rect {
                        layer_id,
                        sprite_id,
                        ..
                    }
                    | globals::ObjectBackend::String {
                        layer_id,
                        sprite_id,
                        ..
                    }
                    | globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        ..
                    } => {
                        if Some(*layer_id) == ui_layer {
                            continue;
                        }
                        if let Some(layer) = self.layers.layer_mut(*layer_id) {
                            if let Some(spr) = layer.sprite_mut(*sprite_id) {
                                spr.visible = false;
                            }
                        }
                    }
                    globals::ObjectBackend::Number {
                        layer_id,
                        sprite_ids,
                    } => {
                        if Some(*layer_id) == ui_layer {
                            continue;
                        }
                        if let Some(layer) = self.layers.layer_mut(*layer_id) {
                            for sid in sprite_ids {
                                if let Some(spr) = layer.sprite_mut(*sid) {
                                    spr.visible = false;
                                }
                            }
                        }
                    }
                    globals::ObjectBackend::Gfx => {
                        if let Some((lid, sid)) =
                            self.gfx.object_sprite_binding(*stage_idx, obj_idx as i64)
                        {
                            if Some(lid) == ui_layer {
                                continue;
                            }
                            if let Some(layer) = self.layers.layer_mut(lid) {
                                if let Some(spr) = layer.sprite_mut(sid) {
                                    spr.visible = false;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_syscom_menu_key(&mut self, k: input::VmKey) -> bool {
        if !self.globals.syscom.menu_open {
            return false;
        }
        match k {
            input::VmKey::Escape | input::VmKey::Enter => {
                self.close_syscom_menu();
                return true;
            }
            input::VmKey::ArrowUp => {
                let len = self.menu_items().len();
                if len > 0 {
                    let c = self.globals.syscom.menu_cursor;
                    self.globals.syscom.menu_cursor = if c == 0 { len - 1 } else { c - 1 };
                }
                return true;
            }
            input::VmKey::ArrowDown => {
                let len = self.menu_items().len();
                if len > 0 {
                    self.globals.syscom.menu_cursor = (self.globals.syscom.menu_cursor + 1) % len;
                }
                return true;
            }
            input::VmKey::ArrowLeft => {
                self.menu_adjust(-1);
                return true;
            }
            input::VmKey::ArrowRight => {
                self.menu_adjust(1);
                return true;
            }
            input::VmKey::Digit(d) => {
                let idx = d as usize;
                if self.handle_save_load_digit(idx) {
                    return true;
                }
            }
            _ => {}
        }
        true
    }

    fn handle_syscom_menu_click(&mut self) -> bool {
        if !self.globals.syscom.menu_open {
            return false;
        }
        self.close_syscom_menu();
        true
    }

    fn close_syscom_menu(&mut self) {
        self.globals.syscom.menu_open = false;
        self.globals.syscom.menu_kind = None;
    }

    fn handle_save_load_digit(&mut self, idx: usize) -> bool {
        if let Some(kind) = self.globals.syscom.menu_kind {
            const CALL_SAVE_MENU: i32 = 86;
            const CALL_LOAD_MENU: i32 = 92;
            const QUICK_SAVE: i32 = 100;
            const QUICK_LOAD: i32 = 101;
            if kind == CALL_SAVE_MENU {
                syscom_form::menu_save_slot(self, false, idx);
            } else if kind == CALL_LOAD_MENU {
                syscom_form::menu_load_slot(self, false, idx);
            } else if kind == QUICK_SAVE {
                syscom_form::menu_save_slot(self, true, idx);
            } else if kind == QUICK_LOAD {
                syscom_form::menu_load_slot(self, true, idx);
            } else {
                return false;
            }
            self.globals.syscom.menu_result = Some(idx as i64);
            self.globals.syscom.system_extra_int_value = idx as i64;
            self.close_syscom_menu();
            return true;
        }
        false
    }

    fn menu_items(&mut self) -> Vec<MenuItem> {
        syscom_menu_items(&mut self.globals.syscom, &self.project_dir)
    }

    fn menu_adjust(&mut self, dir: i32) {
        let mut items = self.menu_items();
        if items.is_empty() {
            return;
        }
        let idx = self.globals.syscom.menu_cursor.min(items.len() - 1);
        match items.get_mut(idx) {
            Some(MenuItem::Int {
                key,
                min,
                max,
                step,
                ..
            }) => {
                let cur = self
                    .globals
                    .syscom
                    .config_int
                    .get(key)
                    .copied()
                    .unwrap_or(*min as i64);
                let next = (cur + (*step as i64 * dir as i64)).clamp(*min as i64, *max as i64);
                self.globals.syscom.config_int.insert(*key, next);
                if *key == GET_ALL_VOLUME
                    || *key == GET_BGM_VOLUME
                    || *key == GET_KOE_VOLUME
                    || *key == GET_PCM_VOLUME
                    || *key == GET_SE_VOLUME
                    || *key == GET_MOV_VOLUME
                {
                    syscom_form::apply_audio_config(self);
                }
            }
            Some(MenuItem::Bool { key, .. }) => {
                let cur = self
                    .globals
                    .syscom
                    .config_int
                    .get(key)
                    .copied()
                    .unwrap_or(0);
                let next = if cur == 0 { 1 } else { 0 };
                self.globals.syscom.config_int.insert(*key, next);
                if *key == GET_ALL_ONOFF
                    || *key == GET_BGM_ONOFF
                    || *key == GET_KOE_ONOFF
                    || *key == GET_PCM_ONOFF
                    || *key == GET_SE_ONOFF
                    || *key == GET_MOV_ONOFF
                {
                    syscom_form::apply_audio_config(self);
                }
            }
            Some(MenuItem::FontName) => {
                let list = self.globals.syscom.font_list.clone();
                if list.is_empty() {
                    return;
                }
                let cur = self
                    .globals
                    .syscom
                    .config_str
                    .get(&GET_FONT_NAME)
                    .cloned()
                    .unwrap_or_default();
                let mut pos = list.iter().position(|s| s == &cur).unwrap_or(0) as i32;
                pos += dir;
                if pos < 0 {
                    pos = list.len() as i32 - 1;
                }
                let pos = (pos as usize) % list.len();
                self.globals
                    .syscom
                    .config_str
                    .insert(GET_FONT_NAME, list[pos].clone());
            }
            None => {}
        }
    }

    fn sync_syscom_menu_ui(&mut self) {
        if !self.globals.syscom.menu_open {
            self.ui.set_sys_overlay(false, String::new());
            return;
        }
        let len = self.menu_items().len();
        if len > 0 && self.globals.syscom.menu_cursor >= len {
            self.globals.syscom.menu_cursor = 0;
        }
        let text = build_syscom_menu_text(&mut self.globals.syscom, &self.project_dir);
        self.ui.set_sys_overlay(true, text);
    }

    fn handle_mwnd_selection_key(&mut self, k: input::VmKey) -> bool {
        let Some((form_id, stage_idx, mwnd_idx)) = self.globals.focused_stage_mwnd else {
            return false;
        };
        let mut clear_focus = false;
        let mut handled = false;
        if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
            if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
                if let Some(m) = list.get_mut(mwnd_idx) {
                    if let Some(sel) = m.selection.as_mut() {
                        handled = match k {
                            input::VmKey::ArrowUp => {
                                if !sel.choices.is_empty() {
                                    sel.cursor = if sel.cursor == 0 {
                                        sel.choices.len() - 1
                                    } else {
                                        sel.cursor - 1
                                    };
                                }
                                true
                            }
                            input::VmKey::ArrowDown => {
                                if !sel.choices.is_empty() {
                                    sel.cursor = (sel.cursor + 1) % sel.choices.len();
                                }
                                true
                            }
                            input::VmKey::Enter => {
                                sel.result = (sel.cursor as i64) + 1;
                                clear_focus = true;
                                true
                            }
                            input::VmKey::Escape if sel.cancel_enable => {
                                sel.result = -1;
                                clear_focus = true;
                                true
                            }
                            _ => false,
                        };
                    } else {
                        clear_focus = true;
                    }
                } else {
                    clear_focus = true;
                }
            } else {
                clear_focus = true;
            }
        } else {
            clear_focus = true;
        }
        if clear_focus {
            self.globals.focused_stage_mwnd = None;
        }
        handled
    }

    fn handle_mwnd_selection_click(&mut self, b: input::VmMouseButton) -> bool {
        let Some((form_id, stage_idx, mwnd_idx)) = self.globals.focused_stage_mwnd else {
            return false;
        };
        let mut clear_focus = false;
        let mut handled = false;
        if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
            if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
                if let Some(m) = list.get_mut(mwnd_idx) {
                    if let Some(sel) = m.selection.as_mut() {
                        handled = match b {
                            input::VmMouseButton::Left => {
                                sel.result = (sel.cursor as i64) + 1;
                                clear_focus = true;
                                true
                            }
                            input::VmMouseButton::Right if sel.cancel_enable => {
                                sel.result = -1;
                                clear_focus = true;
                                true
                            }
                            _ => false,
                        };
                    } else {
                        clear_focus = true;
                    }
                } else {
                    clear_focus = true;
                }
            } else {
                clear_focus = true;
            }
        } else {
            clear_focus = true;
        }
        if clear_focus {
            self.globals.focused_stage_mwnd = None;
        }
        handled
    }

    fn sync_mwnd_selection_ui(&mut self) {
        if self.globals.syscom.menu_open {
            return;
        }
        let text = if let Some((form_id, stage_idx, mwnd_idx)) = self.globals.focused_stage_mwnd {
            self.globals
                .stage_forms
                .get(&form_id)
                .and_then(|st| st.mwnd_lists.get(&stage_idx))
                .and_then(|list| list.get(mwnd_idx))
                .and_then(|m| m.selection.as_ref())
                .map(|sel| {
                    let mut lines = Vec::new();
                    lines.push("Select".to_string());
                    for (i, choice) in sel.choices.iter().enumerate() {
                        let cursor = if i == sel.cursor { ">" } else { " " };
                        lines.push(format!("{cursor} {}", choice.text));
                    }
                    if sel.cancel_enable {
                        lines.push("[Esc] Cancel".to_string());
                    }
                    lines.join("\n")
                })
        } else {
            None
        };
        if let Some(text) = text {
            self.ui.set_sys_overlay(true, text);
        } else {
            self.ui.set_sys_overlay(false, String::new());
        }
    }

    fn sync_movie_objects(&mut self) {
        let (globals, layers, movie_mgr, audio, gfx, images, ids) = (
            &mut self.globals,
            &mut self.layers,
            &mut self.movie,
            &mut self.audio,
            &mut self.gfx,
            &mut self.images,
            &self.ids,
        );
        for st in globals.stage_forms.values_mut() {
            for (stage_idx, objs) in st.object_lists.iter_mut() {
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    if !obj.used || obj.object_type != 9 {
                        continue;
                    }
                    let Some(file) = obj.file_name.as_deref() else {
                        continue;
                    };

                    if obj.movie.just_finished {
                        if let Some(id) = obj.movie.audio_id.take() {
                            movie_mgr.stop_audio(id);
                        }
                        obj.movie.just_finished = false;
                        if obj.movie.auto_free_flag {
                            if let globals::ObjectBackend::Movie {
                                layer_id,
                                sprite_id,
                                ..
                            } = obj.backend
                            {
                                if let Some(layer) = layers.layer_mut(layer_id) {
                                    if let Some(sprite) = layer.sprite_mut(sprite_id) {
                                        sprite.visible = false;
                                        sprite.image_id = None;
                                    }
                                }
                            }
                            obj.init_type_like();
                            continue;
                        }
                    } else if !obj.movie.playing {
                        if let Some(id) = obj.movie.audio_id.take() {
                            movie_mgr.stop_audio(id);
                        }
                    }

                    let (layer_id, sprite_id) = if let globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        ..
                    } = &obj.backend
                    {
                        (*layer_id, *sprite_id)
                    } else {
                        let Some(layer_id) = gfx.ensure_stage_layer_id(layers, *stage_idx) else {
                            continue;
                        };
                        let Some(layer) = layers.layer_mut(layer_id) else {
                            continue;
                        };
                        let sid = layer.create_sprite();
                        if let Some(sprite) = layer.sprite_mut(sid) {
                            sprite.visible = true;
                            sprite.alpha = 255;
                            sprite.fit = SpriteFit::PixelRect;
                            sprite.size_mode = SpriteSizeMode::Intrinsic;
                            sprite.x = 0;
                            sprite.y = 0;
                            sprite.order = 0;
                        }
                        obj.backend = globals::ObjectBackend::Movie {
                            layer_id,
                            sprite_id: sid,
                            image_id: None,
                            width: 0,
                            height: 0,
                        };
                        (layer_id, sid)
                    };

                    if let Some(layer) = layers.layer_mut(layer_id) {
                        if let Some(sprite) = layer.sprite_mut(sprite_id) {
                            let disp = if ids.obj_disp != 0 {
                                obj.extra_int_props.get(&ids.obj_disp).copied().unwrap_or(1)
                            } else {
                                1
                            };
                            sprite.visible = disp != 0;
                            if ids.obj_x != 0 {
                                if let Some(v) = obj.extra_int_props.get(&ids.obj_x) {
                                    sprite.x = *v as i32;
                                }
                            }
                            if ids.obj_y != 0 {
                                if let Some(v) = obj.extra_int_props.get(&ids.obj_y) {
                                    sprite.y = *v as i32;
                                }
                            }
                            if ids.obj_alpha != 0 {
                                if let Some(v) = obj.extra_int_props.get(&ids.obj_alpha) {
                                    sprite.alpha = (*v).clamp(0, 255) as u8;
                                }
                            }
                            if ids.obj_order != 0 {
                                if let Some(v) = obj.extra_int_props.get(&ids.obj_order) {
                                    sprite.order = *v as i32;
                                }
                            }
                        }
                    }

                    if obj.movie.seeked || obj.movie.just_looped {
                        if let Some(id) = obj.movie.audio_id.take() {
                            movie_mgr.stop_audio(id);
                        }
                    }
                    obj.movie.seeked = false;
                    obj.movie.just_looped = false;

                    if obj.movie.pause_flag {
                        if let Some(id) = obj.movie.audio_id {
                            movie_mgr.pause_audio(id);
                        }
                    } else if obj.movie.playing {
                        if let Some(id) = obj.movie.audio_id {
                            movie_mgr.resume_audio(id);
                        }
                    }

                    if obj.movie.playing && !obj.movie.pause_flag && obj.movie.audio_id.is_none() {
                        let asset_audio = match movie_mgr.ensure_asset(file) {
                            Ok((a, _)) => a.audio.clone(),
                            Err(_) => None,
                        };
                        if let Some(track) = asset_audio.as_ref() {
                            if let Ok(id) = movie_mgr.start_audio(audio, track, obj.movie.timer_ms)
                            {
                                obj.movie.audio_id = Some(id);
                            }
                        }
                    }

                    let asset = match movie_mgr.ensure_asset(file) {
                        Ok((a, _)) => a,
                        Err(_) => continue,
                    };

                    if asset.frames.is_empty() {
                        continue;
                    }
                    let fps = asset.info.fps.unwrap_or(0.0);
                    if fps <= 0.0 {
                        continue;
                    }
                    let mut frame_idx =
                        ((obj.movie.timer_ms as f64) * (fps as f64) / 1000.0).floor() as usize;
                    if obj.movie.loop_flag {
                        frame_idx %= asset.frames.len();
                    } else if frame_idx >= asset.frames.len() {
                        frame_idx = asset.frames.len() - 1;
                    }
                    if obj.movie.last_frame_idx == Some(frame_idx) {
                        continue;
                    }
                    obj.movie.last_frame_idx = Some(frame_idx);

                    let frame = asset.frames[frame_idx].clone();
                    if let globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        image_id,
                        width,
                        height,
                    } = &mut obj.backend
                    {
                        let img_id = match image_id {
                            Some(id) => {
                                let _ = images.replace_image_arc(*id, frame.clone());
                                *id
                            }
                            None => {
                                let id = images.insert_image_arc(frame.clone());
                                if let Some(layer) = layers.layer_mut(*layer_id) {
                                    if let Some(sprite) = layer.sprite_mut(*sprite_id) {
                                        sprite.image_id = Some(id);
                                    }
                                }
                                *image_id = Some(id);
                                id
                            }
                        };
                        if let Some(layer) = layers.layer_mut(*layer_id) {
                            if let Some(sprite) = layer.sprite_mut(*sprite_id) {
                                sprite.image_id = Some(img_id);
                            }
                        }
                        *width = frame.width;
                        *height = frame.height;
                    }
                }
            }
        }
    }

    /// Build a render list and apply screen/wipe effects (bring-up level).
    pub fn render_list_with_effects(&mut self) -> Vec<RenderSprite> {
        self.apply_object_masks();
        let mut list = self.layers.render_list();
        apply_quake(&self.globals, &mut list);
        apply_button_visuals(self, &mut list);
        self.apply_gan_effects(&mut list);
        apply_screen_effects(&self.globals, &self.ids, &mut list);
        apply_wipe_effect(self, &mut list);
        apply_syscom_filter(self, &mut list);
        list
    }

    /// Capture the current frame (UI + scene) into a CPU RGBA buffer.
    pub fn capture_frame_rgba(&mut self) -> RgbaImage {
        let sprites = self.render_list_with_effects();
        soft_render::render_to_image(&self.images, &sprites, self.screen_w, self.screen_h)
    }
}

fn trace_codes_enabled() -> bool {
    std::env::var_os("SIGLUS_TRACE_CODES").is_some()
}

pub fn dispatch(ctx: &mut CommandContext, cmd: &Command) -> Result<()> {
    // Numeric dispatch first (forms). If we don't recognize it yet,
    // we record it and keep going.
    if let Some(code) = cmd.code {
        if trace_codes_enabled() {
            let mut elem = String::new();
            for arg in &cmd.args {
                if let Value::Element(chain) = arg {
                    elem = format!(" element={chain:?}");
                    break;
                }
            }
            eprintln!(
                "[TRACE code] form={} args={}{}",
                code.id,
                cmd.args.len(),
                elem
            );
        }
        if opcode::dispatch_code(ctx, code, &cmd.args)? {
            return Ok(());
        }
        record_unknown_form_chain(ctx, code.id, &cmd.args);
        ctx.unknown.record_code(code);
        return Ok(());
    }

    // Named command dispatch (runtime tools).
    if commands::misc::handle(ctx, cmd)? {
        return Ok(());
    }
    if commands::text::handle(ctx, cmd)? {
        return Ok(());
    }
    if commands::audio::handle(ctx, cmd)? {
        return Ok(());
    }
    if commands::bg::handle(ctx, cmd)? {
        return Ok(());
    }
    if commands::chr::handle(ctx, cmd)? {
        return Ok(());
    }
    if commands::layer::handle(ctx, cmd)? {
        return Ok(());
    }

    // Unknown named command: keep going, but record it for RE.
    ctx.unknown.record_name(&cmd.name);

    Ok(())
}

fn record_unknown_form_chain(ctx: &mut CommandContext, form_id: u32, args: &[Value]) {
    for v in args {
        if let Value::Element(chain) = v {
            ctx.unknown
                .record_element_chain(form_id, chain.as_slice(), "unhandled");
            return;
        }
    }
}

fn apply_button_visuals(ctx: &CommandContext, sprites: &mut [RenderSprite]) {
    use globals::ObjectBackend;
    const TNM_BTN_STATE_NORMAL: i64 = 0;
    const TNM_BTN_STATE_HIT: i64 = 1;
    const TNM_BTN_STATE_PUSH: i64 = 2;
    const TNM_BTN_STATE_SELECT: i64 = 3;
    const TNM_BTN_STATE_DISABLE: i64 = 4;

    let mut map: HashMap<(LayerId, SpriteId), i64> = HashMap::new();

    for st in ctx.globals.stage_forms.values() {
        for (stage_idx, objs) in &st.object_lists {
            for (obj_idx, obj) in objs.iter().enumerate() {
                if !obj.button.enabled && obj.button.state != TNM_BTN_STATE_DISABLE {
                    continue;
                }
                let mut state = obj.button.state;

                if obj.button.is_disabled() {
                    state = TNM_BTN_STATE_DISABLE;
                } else if state != TNM_BTN_STATE_SELECT && state != TNM_BTN_STATE_DISABLE {
                    if let Some(gidx) = obj.button.group_idx() {
                        if let Some(groups) = st.group_lists.get(stage_idx) {
                            if let Some(gl) = groups.get(gidx) {
                                if gl.decided_button_no == obj.button.button_no {
                                    state = TNM_BTN_STATE_PUSH;
                                } else if gl.pushed_button_no == obj.button.button_no {
                                    state = TNM_BTN_STATE_PUSH;
                                } else if gl.hit_button_no == obj.button.button_no {
                                    state = TNM_BTN_STATE_HIT;
                                }
                            }
                        }
                    } else if obj.button.pushed {
                        state = TNM_BTN_STATE_PUSH;
                    } else if obj.button.hit {
                        state = TNM_BTN_STATE_HIT;
                    }
                }

                match &obj.backend {
                    ObjectBackend::Gfx => {
                        if let Some((lid, sid)) =
                            ctx.gfx.object_sprite_binding(*stage_idx, obj_idx as i64)
                        {
                            map.insert((lid, sid), state);
                        }
                    }
                    ObjectBackend::Rect {
                        layer_id,
                        sprite_id,
                        ..
                    } => {
                        map.insert((*layer_id, *sprite_id), state);
                    }
                    ObjectBackend::String {
                        layer_id,
                        sprite_id,
                        ..
                    } => {
                        map.insert((*layer_id, *sprite_id), state);
                    }
                    ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        ..
                    } => {
                        map.insert((*layer_id, *sprite_id), state);
                    }
                    ObjectBackend::Number {
                        layer_id,
                        sprite_ids,
                    } => {
                        for sid in sprite_ids {
                            map.insert((*layer_id, *sid), state);
                        }
                    }
                    ObjectBackend::None => {}
                }
            }
        }
    }

    if map.is_empty() {
        return;
    }

    for rs in sprites.iter_mut() {
        let (Some(lid), Some(sid)) = (rs.layer_id, rs.sprite_id) else {
            continue;
        };
        let Some(state) = map.get(&(lid, sid)).copied() else {
            continue;
        };
        apply_button_state_visual(&mut rs.sprite, state);
    }
}

fn apply_button_state_visual(sprite: &mut Sprite, state: i64) {
    const TNM_BTN_STATE_NORMAL: i64 = 0;
    const TNM_BTN_STATE_HIT: i64 = 1;
    const TNM_BTN_STATE_PUSH: i64 = 2;
    const TNM_BTN_STATE_SELECT: i64 = 3;
    const TNM_BTN_STATE_DISABLE: i64 = 4;

    match state {
        TNM_BTN_STATE_DISABLE => {
            sprite.alpha = ((sprite.alpha as u16 * 128) / 255) as u8;
            sprite.mono = sprite.mono.saturating_add(120);
        }
        TNM_BTN_STATE_HIT => {
            sprite.bright = sprite.bright.saturating_add(40);
        }
        TNM_BTN_STATE_PUSH => {
            sprite.dark = sprite.dark.saturating_add(50);
        }
        TNM_BTN_STATE_SELECT => {
            sprite.bright = sprite.bright.saturating_add(20);
        }
        TNM_BTN_STATE_NORMAL | _ => {}
    }
}

fn apply_quake(globals: &globals::GlobalState, sprites: &mut [RenderSprite]) {
    let mut dx_total: f32 = 0.0;
    let mut dy_total: f32 = 0.0;
    for st in globals.screen_forms.values() {
        if let Some(t) = st.shake_until {
            if std::time::Instant::now() < t {
                let power = 6.0f32;
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as f32;
                dx_total += (ms * 0.021).sin() * power;
                dy_total += (ms * 0.019).cos() * power;
            }
        }
        for selector in &st.quake_selectors {
            if let Some(list) = st.lists.get(selector) {
                for item in &list.items {
                    if item.quake_until.is_some() {
                        let mut power = item.quake_power;
                        if power == 0 {
                            power = item.props.values().next().copied().unwrap_or(6) as i32;
                        }
                        if power <= 0 {
                            continue;
                        }
                        let power = power.min(32) as f32;
                        let t = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as f32;
                        let mut dx = (t * 0.02).sin() * power;
                        let mut dy = (t * 0.017).cos() * power;
                        if item.quake_vec != 0 {
                            let angle = (item.quake_vec as f32) * std::f32::consts::PI / 180.0;
                            let (s, c) = angle.sin_cos();
                            let rx = dx * c - dy * s;
                            let ry = dx * s + dy * c;
                            dx = rx;
                            dy = ry;
                        }
                        dx_total += dx;
                        dy_total += dy;
                    }
                }
            }
        }
    }
    if dx_total == 0.0 && dy_total == 0.0 {
        return;
    }

    for rs in sprites.iter_mut() {
        rs.sprite.x = rs.sprite.x.saturating_add(dx_total as i32);
        rs.sprite.y = rs.sprite.y.saturating_add(dy_total as i32);
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectParam {
    x: i32,
    y: i32,
    mono: i32,
    reverse: i32,
    bright: i32,
    dark: i32,
    color_r: i32,
    color_g: i32,
    color_b: i32,
    color_rate: i32,
    color_add_r: i32,
    color_add_g: i32,
    color_add_b: i32,
    begin_order: i32,
    begin_layer: i32,
    end_order: i32,
    end_layer: i32,
}

fn apply_screen_effects(
    globals: &globals::GlobalState,
    ids: &id_map::IdMap,
    sprites: &mut [RenderSprite],
) {
    let effects = collect_screen_effects(globals, ids);
    if effects.is_empty() {
        return;
    }
    for effect in &effects {
        for rs in sprites.iter_mut() {
            let layer = rs.layer_id.map(|v| v as i32).unwrap_or(0);
            let order = rs.sprite.order;
            if !in_range(layer, order, effect) {
                continue;
            }
            apply_effect_to_sprite(&mut rs.sprite, effect);
        }
    }
}

fn collect_screen_effects(globals: &globals::GlobalState, ids: &id_map::IdMap) -> Vec<EffectParam> {
    let mut out = Vec::new();
    for st in globals.screen_forms.values() {
        for (selector, list) in &st.lists {
            if st.quake_selectors.contains(selector) {
                continue;
            }
            for item in &list.items {
                if let Some(effect) = effect_from_screen_item(item, ids) {
                    out.push(effect);
                }
            }
        }
    }
    out
}

fn effect_from_screen_item(
    item: &globals::ScreenItemState,
    ids: &id_map::IdMap,
) -> Option<EffectParam> {
    let has_ids = ids.effect_x != 0
        || ids.effect_y != 0
        || ids.effect_z != 0
        || ids.effect_mono != 0
        || ids.effect_reverse != 0
        || ids.effect_bright != 0
        || ids.effect_dark != 0
        || ids.effect_color_r != 0
        || ids.effect_color_g != 0
        || ids.effect_color_b != 0
        || ids.effect_color_rate != 0
        || ids.effect_color_add_r != 0
        || ids.effect_color_add_g != 0
        || ids.effect_color_add_b != 0
        || ids.effect_begin_order != 0
        || ids.effect_end_order != 0
        || ids.effect_begin_layer != 0
        || ids.effect_end_layer != 0;
    if has_ids {
        let read = |op: i32| -> i64 {
            if op == 0 {
                return 0;
            }
            if let Some(ev) = item.events.get(&op) {
                return ev.get_total_value() as i64;
            }
            item.props.get(&op).copied().unwrap_or(0)
        };
        let mut effect = EffectParam {
            x: read(ids.effect_x) as i32,
            y: read(ids.effect_y) as i32,
            mono: read(ids.effect_mono) as i32,
            reverse: read(ids.effect_reverse) as i32,
            bright: read(ids.effect_bright) as i32,
            dark: read(ids.effect_dark) as i32,
            color_r: read(ids.effect_color_r) as i32,
            color_g: read(ids.effect_color_g) as i32,
            color_b: read(ids.effect_color_b) as i32,
            color_rate: read(ids.effect_color_rate) as i32,
            color_add_r: read(ids.effect_color_add_r) as i32,
            color_add_g: read(ids.effect_color_add_g) as i32,
            color_add_b: read(ids.effect_color_add_b) as i32,
            begin_order: read(ids.effect_begin_order) as i32,
            begin_layer: read(ids.effect_begin_layer) as i32,
            end_order: read(ids.effect_end_order) as i32,
            end_layer: read(ids.effect_end_layer) as i32,
        };
        if effect.begin_layer == 0 && effect.end_layer == 0 {
            effect.begin_layer = i32::MIN;
            effect.end_layer = i32::MAX;
        }
        if !effect_is_use(&effect) {
            return None;
        }
        return Some(effect);
    }

    let mut vals = vec![0i64; 20];
    if !item.confirmed_event_ops.is_empty() {
        for (i, op) in item.confirmed_event_ops.iter().enumerate().take(vals.len()) {
            if let Some(ev) = item.events.get(op) {
                vals[i] = ev.get_total_value() as i64;
            }
        }
    } else if !item.props.is_empty() {
        let mut props: Vec<(i32, i64)> = item.props.iter().map(|(k, v)| (*k, *v)).collect();
        props.sort_by_key(|(k, _)| *k);
        for (i, (_, v)) in props.into_iter().enumerate().take(vals.len()) {
            vals[i] = v;
        }
    }

    let mut effect = EffectParam {
        x: vals.get(0).copied().unwrap_or(0) as i32,
        y: vals.get(1).copied().unwrap_or(0) as i32,
        mono: vals.get(3).copied().unwrap_or(0) as i32,
        reverse: vals.get(4).copied().unwrap_or(0) as i32,
        bright: vals.get(5).copied().unwrap_or(0) as i32,
        dark: vals.get(6).copied().unwrap_or(0) as i32,
        color_r: vals.get(7).copied().unwrap_or(0) as i32,
        color_g: vals.get(8).copied().unwrap_or(0) as i32,
        color_b: vals.get(9).copied().unwrap_or(0) as i32,
        color_rate: vals.get(10).copied().unwrap_or(0) as i32,
        color_add_r: vals.get(11).copied().unwrap_or(0) as i32,
        color_add_g: vals.get(12).copied().unwrap_or(0) as i32,
        color_add_b: vals.get(13).copied().unwrap_or(0) as i32,
        begin_order: vals.get(14).copied().unwrap_or(0) as i32,
        begin_layer: vals.get(15).copied().map(|v| v as i32).unwrap_or(i32::MIN),
        end_order: vals.get(16).copied().unwrap_or(0) as i32,
        end_layer: vals.get(17).copied().map(|v| v as i32).unwrap_or(i32::MAX),
    };

    // Defaults when only a subset of fields was provided.
    if item.confirmed_event_ops.is_empty() && item.props.is_empty() {
        return None;
    }

    if effect.begin_layer == 0 && effect.end_layer == 0 {
        effect.begin_layer = i32::MIN;
        effect.end_layer = i32::MAX;
    }

    if !effect_is_use(&effect) {
        return None;
    }
    Some(effect)
}

fn effect_is_use(effect: &EffectParam) -> bool {
    effect.x != 0
        || effect.y != 0
        || effect.mono != 0
        || effect.reverse != 0
        || effect.bright != 0
        || effect.dark != 0
        || effect.color_r != 0
        || effect.color_g != 0
        || effect.color_b != 0
        || effect.color_rate != 0
        || effect.color_add_r != 0
        || effect.color_add_g != 0
        || effect.color_add_b != 0
}

fn in_range(layer: i32, order: i32, effect: &EffectParam) -> bool {
    let (lo_layer, hi_layer) = if effect.begin_layer <= effect.end_layer {
        (effect.begin_layer, effect.end_layer)
    } else {
        (effect.end_layer, effect.begin_layer)
    };
    let (lo_order, hi_order) = if effect.begin_order <= effect.end_order {
        (effect.begin_order, effect.end_order)
    } else {
        (effect.end_order, effect.begin_order)
    };
    layer >= lo_layer && layer <= hi_layer && order >= lo_order && order <= hi_order
}

fn apply_effect_to_sprite(sprite: &mut Sprite, effect: &EffectParam) {
    sprite.x = sprite.x.saturating_add(effect.x);
    sprite.y = sprite.y.saturating_add(effect.y);

    sprite.mono = combine_lerp(sprite.mono, effect.mono);
    sprite.reverse = combine_lerp(sprite.reverse, effect.reverse);
    sprite.bright = combine_lerp(sprite.bright, effect.bright);
    sprite.dark = combine_lerp(sprite.dark, effect.dark);

    // Color rate uses the original blend formula.
    let sr = sprite.color_rate as i32;
    let pr = clamp_u8(effect.color_rate);
    if sr + pr > 0 {
        let parent_rate = (pr * 255 * 255) / (255 * 255 - (255 - sr) * (255 - pr));
        sprite.color_r = blend_color(sprite.color_r, effect.color_r, parent_rate);
        sprite.color_g = blend_color(sprite.color_g, effect.color_g, parent_rate);
        sprite.color_b = blend_color(sprite.color_b, effect.color_b, parent_rate);
    }
    sprite.color_rate = (255 - (255 - sr) * (255 - pr) / 255) as u8;

    sprite.color_add_r = clamp_add(sprite.color_add_r, effect.color_add_r);
    sprite.color_add_g = clamp_add(sprite.color_add_g, effect.color_add_g);
    sprite.color_add_b = clamp_add(sprite.color_add_b, effect.color_add_b);
}

fn combine_lerp(base: u8, parent: i32) -> u8 {
    let parent = clamp_u8(parent);
    (255 - (255 - base as i32) * (255 - parent) / 255) as u8
}

fn blend_color(base: u8, parent: i32, rate: i32) -> u8 {
    let parent = clamp_u8(parent);
    let base = base as i32;
    ((base * (255 - rate) + parent * rate) / 255) as u8
}

fn clamp_u8(v: i32) -> i32 {
    v.clamp(0, 255)
}

fn clamp_add(base: u8, add: i32) -> u8 {
    let v = base as i32 + add;
    v.clamp(0, 255) as u8
}

fn apply_wipe_effect(ctx: &mut CommandContext, sprites: &mut [RenderSprite]) {
    let Some(wipe) = ctx.globals.wipe.as_mut() else {
        return;
    };
    let mut mask_cache = std::mem::take(&mut wipe.mask_cache);
    let mask_file = wipe.mask_file.clone();
    let mask_image_id = wipe.mask_image_id;
    let wipe_type = wipe.wipe_type;
    let speed_mode = wipe.speed_mode;
    let option = wipe.option.clone();
    let begin_layer = wipe.begin_layer;
    let end_layer = wipe.end_layer;
    let begin_order = wipe.begin_order;
    let end_order = wipe.end_order;
    let with_low = wipe.with_low_order != 0;
    let mut progress = wipe.progress();
    let _ = wipe;
    progress = match speed_mode {
        1 => progress * progress,
        2 => 1.0 - (1.0 - progress) * (1.0 - progress),
        3 => progress * progress * (3.0 - 2.0 * progress),
        _ => progress,
    };
    let fade = (progress * 255.0).clamp(0.0, 255.0) as u8;

    for rs in sprites.iter_mut() {
        let layer = rs.layer_id.map(|v| v as i32).unwrap_or(0);
        let order = rs.sprite.order;
        if layer < begin_layer || layer > end_layer {
            continue;
        }
        if !with_low && (order < begin_order || order > end_order) {
            continue;
        }
        if with_low && order < begin_order {
            // Include lower orders when requested.
        } else if order < begin_order || order > end_order {
            continue;
        }

        if mask_file.is_some() {
            let Some(mask_id) = mask_image_id else {
                continue;
            };
            let reverse = option.get(0).copied().unwrap_or(0) != 0;
            let t = if reverse { 1.0 - progress } else { progress };
            let bucket = (t * 255.0).round().clamp(0.0, 255.0) as u16;

            if let Some(base_id) = rs.sprite.image_id {
                let Some((base_img, base_ver)) = ctx.images.get_entry(base_id) else {
                    continue;
                };
                let Some((mask_img, mask_ver)) = ctx.images.get_entry(mask_id) else {
                    continue;
                };

                let key = (base_id, base_ver, mask_id, mask_ver, bucket);
                if let Some(&masked_id) = mask_cache.get(&key) {
                    rs.sprite.image_id = Some(masked_id);
                } else {
                    let masked = apply_wipe_mask_image(base_img, mask_img, t);
                    let masked_id = ctx.images.insert_image(masked);
                    mask_cache.insert(key, masked_id);
                    rs.sprite.image_id = Some(masked_id);
                }
            }

            rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
            continue;
        }

        match wipe_type {
            1 | 2 | 3 | 4 | 5 | 6 => {
                if let Some((left, top, right, bottom)) = sprite_bounds(&rs.sprite, ctx) {
                    let w = (right - left).max(1);
                    let h = (bottom - top).max(1);
                    let clip = match wipe_type {
                        1 => {
                            let x = left + ((w as f32) * progress) as i32;
                            ClipRect {
                                left,
                                top,
                                right: x,
                                bottom,
                            }
                        }
                        2 => {
                            let x = right - ((w as f32) * progress) as i32;
                            ClipRect {
                                left: x,
                                top,
                                right,
                                bottom,
                            }
                        }
                        3 => {
                            let y = top + ((h as f32) * progress) as i32;
                            ClipRect {
                                left,
                                top,
                                right,
                                bottom: y,
                            }
                        }
                        4 => {
                            let y = bottom - ((h as f32) * progress) as i32;
                            ClipRect {
                                left,
                                top: y,
                                right,
                                bottom,
                            }
                        }
                        5 => {
                            let cx = left + w / 2;
                            let cy = top + h / 2;
                            let hw = ((w as f32) * progress / 2.0) as i32;
                            let hh = ((h as f32) * progress / 2.0) as i32;
                            ClipRect {
                                left: cx - hw,
                                top: cy - hh,
                                right: cx + hw,
                                bottom: cy + hh,
                            }
                        }
                        6 => {
                            let cx = left + w / 2;
                            let cy = top + h / 2;
                            let hw = ((w as f32) * (1.0 - progress) / 2.0) as i32;
                            let hh = ((h as f32) * (1.0 - progress) / 2.0) as i32;
                            ClipRect {
                                left: cx - hw,
                                top: cy - hh,
                                right: cx + hw,
                                bottom: cy + hh,
                            }
                        }
                        _ => ClipRect {
                            left,
                            top,
                            right,
                            bottom,
                        },
                    };
                    rs.sprite.dst_clip = Some(clip);
                    rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
                } else {
                    rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
                }
            }
            _ => {
                rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
            }
        }
    }

    if let Some(wipe) = ctx.globals.wipe.as_mut() {
        wipe.mask_cache = mask_cache;
    }
}

fn sprite_bounds(sprite: &Sprite, ctx: &CommandContext) -> Option<(i32, i32, i32, i32)> {
    match sprite.fit {
        crate::layer::SpriteFit::FullScreen => {
            let w = ctx.screen_w as i32;
            let h = ctx.screen_h as i32;
            Some((0, 0, w, h))
        }
        crate::layer::SpriteFit::PixelRect => {
            let (mut w, mut h) = match sprite.size_mode {
                crate::layer::SpriteSizeMode::Explicit { width, height } => {
                    (width as i32, height as i32)
                }
                crate::layer::SpriteSizeMode::Intrinsic => {
                    let Some(id) = sprite.image_id else {
                        return None;
                    };
                    let (img, _) = ctx.images.get_entry(id)?;
                    (img.width as i32, img.height as i32)
                }
            };
            w = ((w as f32) * sprite.scale_x) as i32;
            h = ((h as f32) * sprite.scale_y) as i32;
            let left = sprite.x;
            let top = sprite.y;
            Some((left, top, left + w, top + h))
        }
    }
}

fn apply_syscom_filter(ctx: &CommandContext, sprites: &mut Vec<RenderSprite>) {
    const GET_FILTER_COLOR_R: i32 = 272;
    const GET_FILTER_COLOR_G: i32 = 273;
    const GET_FILTER_COLOR_B: i32 = 274;
    const GET_FILTER_COLOR_A: i32 = 275;
    let cfg = &ctx.globals.syscom.config_int;
    let a = cfg
        .get(&GET_FILTER_COLOR_A)
        .copied()
        .unwrap_or(0)
        .clamp(0, 255) as u8;
    if a == 0 {
        return;
    }
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

    let mut s = crate::layer::Sprite::default();
    s.visible = true;
    s.image_id = Some(ctx.solid_white);
    s.fit = crate::layer::SpriteFit::FullScreen;
    s.size_mode = crate::layer::SpriteSizeMode::Intrinsic;
    s.alpha = a;
    s.color_rate = 255;
    s.color_r = r;
    s.color_g = g;
    s.color_b = b;
    s.order = i32::MAX;
    sprites.push(RenderSprite {
        layer_id: None,
        sprite_id: None,
        sprite: s,
    });
}

fn resolve_mask_path(project_dir: &Path, raw: &str) -> Option<PathBuf> {
    let mut norm = raw.replace('\\', "/");
    let mut candidates = Vec::new();

    if !norm.contains('.') {
        for ext in ["png", "bmp", "jpg", "jpeg", "g00"] {
            candidates.push(project_dir.join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("dat").join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("mask").join(format!("{}.{}", norm, ext)));
        }
    }

    candidates.push(project_dir.join(&norm));
    candidates.push(project_dir.join("dat").join(&norm));
    candidates.push(project_dir.join("mask").join(&norm));

    for c in candidates {
        if c.exists() {
            return Some(c);
        }
    }
    None
}

fn apply_mask_image(base: &RgbaImage, mask: &RgbaImage, mask_x: i32, mask_y: i32) -> RgbaImage {
    let mut out = base.clone();
    let bw = base.width as i32;
    let bh = base.height as i32;
    let mw = mask.width as i32;
    let mh = mask.height as i32;

    for y in 0..bh {
        for x in 0..bw {
            let mx = x + mask_x;
            let my = y + mask_y;
            let mask_alpha = if mx >= 0 && my >= 0 && mx < mw && my < mh {
                let mi = ((my as u32 * mask.width + mx as u32) * 4) as usize;
                let mr = mask.rgba[mi] as f32 / 255.0;
                let mg = mask.rgba[mi + 1] as f32 / 255.0;
                let mb = mask.rgba[mi + 2] as f32 / 255.0;
                let ma = mask.rgba[mi + 3] as f32 / 255.0;
                let l = mr * 0.299 + mg * 0.587 + mb * 0.114;
                (l * ma).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let bi = ((y as u32 * base.width + x as u32) * 4) as usize;
            let ba = out.rgba[bi + 3] as f32 / 255.0;
            let na = (ba * mask_alpha).clamp(0.0, 1.0);
            out.rgba[bi + 3] = (na * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }

    out
}

fn apply_wipe_mask_image(base: &RgbaImage, mask: &RgbaImage, threshold: f32) -> RgbaImage {
    let mut out = base.clone();
    let bw = base.width as i32;
    let bh = base.height as i32;
    let mw = mask.width as i32;
    let mh = mask.height as i32;

    for y in 0..bh {
        for x in 0..bw {
            let mx = (x * mw) / bw;
            let my = (y * mh) / bh;
            let mi = ((my as u32 * mask.width + mx as u32) * 4) as usize;
            let mr = mask.rgba[mi] as f32 / 255.0;
            let mg = mask.rgba[mi + 1] as f32 / 255.0;
            let mb = mask.rgba[mi + 2] as f32 / 255.0;
            let ma = mask.rgba[mi + 3] as f32 / 255.0;
            let l = (mr * 0.299 + mg * 0.587 + mb * 0.114) * ma;

            let bi = ((y as u32 * base.width + x as u32) * 4) as usize;
            if l > threshold {
                out.rgba[bi + 3] = 0;
            }
        }
    }

    out
}

fn build_syscom_menu_text(syscom: &mut globals::SyscomRuntimeState, project_dir: &Path) -> String {
    let kind = syscom.menu_kind.unwrap_or(0);
    match kind {
        1 => "SYSCOM MENU\n(Press Esc/Enter/Click to close)".to_string(),
        86 => {
            let mut s = String::from(
                "SAVE MENU\nPress 0-9 to save slot\n(Press Esc/Enter/Click to close)\n",
            );
            for i in 0..10 {
                let exist = syscom.save_slots.get(i).map(|v| v.exist).unwrap_or(false);
                s.push_str(&format!(
                    "  Slot {}: {}\n",
                    i,
                    if exist { "USED" } else { "EMPTY" }
                ));
            }
            s
        }
        92 => {
            let mut s = String::from(
                "LOAD MENU\nPress 0-9 to load slot\n(Press Esc/Enter/Click to close)\n",
            );
            for i in 0..10 {
                let exist = syscom.save_slots.get(i).map(|v| v.exist).unwrap_or(false);
                s.push_str(&format!(
                    "  Slot {}: {}\n",
                    i,
                    if exist { "USED" } else { "EMPTY" }
                ));
            }
            s
        }
        157 | 158 | 159 | 160 | 161 | 162 | 163 | 164 | 165 | 166 | 167 | 168 | 169 => {
            ensure_font_list(syscom, project_dir);
            build_config_menu_text(syscom)
        }
        _ => "MENU\n(Press Esc/Enter/Click to close)".to_string(),
    }
}

#[derive(Clone)]
enum MenuItem {
    Int {
        label: &'static str,
        key: i32,
        min: i32,
        max: i32,
        step: i32,
    },
    Bool {
        label: &'static str,
        key: i32,
    },
    FontName,
}

const GET_WINDOW_MODE: i32 = 172;
const GET_WINDOW_MODE_SIZE: i32 = 175;
const GET_ALL_VOLUME: i32 = 188;
const GET_BGM_VOLUME: i32 = 191;
const GET_KOE_VOLUME: i32 = 194;
const GET_PCM_VOLUME: i32 = 197;
const GET_SE_VOLUME: i32 = 200;
const GET_MOV_VOLUME: i32 = 210;
const GET_MOV_ONOFF: i32 = 217;
const GET_ALL_ONOFF: i32 = 224;
const GET_BGM_ONOFF: i32 = 227;
const GET_KOE_ONOFF: i32 = 230;
const GET_PCM_ONOFF: i32 = 233;
const GET_SE_ONOFF: i32 = 236;
const GET_MESSAGE_SPEED: i32 = 248;
const GET_AUTO_MODE_MOJI_WAIT: i32 = 254;
const GET_AUTO_MODE_MIN_WAIT: i32 = 257;
const GET_FILTER_COLOR_R: i32 = 272;
const GET_FILTER_COLOR_G: i32 = 273;
const GET_FILTER_COLOR_B: i32 = 274;
const GET_FILTER_COLOR_A: i32 = 275;
const GET_NO_WIPE_ANIME_ONOFF: i32 = 296;
const GET_SKIP_WIPE_ANIME_ONOFF: i32 = 299;
const GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 305;
const GET_KOE_DONT_STOP_ONOFF: i32 = 308;
const GET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = 311;
const GET_PLAY_SILENT_SOUND_ONOFF: i32 = 314;
const GET_FONT_NAME: i32 = 318;

fn syscom_menu_items(
    syscom: &mut globals::SyscomRuntimeState,
    project_dir: &Path,
) -> Vec<MenuItem> {
    let kind = syscom.menu_kind.unwrap_or(0);
    match kind {
        158 => vec![
            MenuItem::Bool {
                label: "WINDOW_MODE",
                key: GET_WINDOW_MODE,
            },
            MenuItem::Int {
                label: "WINDOW_SIZE",
                key: GET_WINDOW_MODE_SIZE,
                min: 0,
                max: 7,
                step: 1,
            },
        ],
        159 => vec![
            MenuItem::Int {
                label: "ALL_VOL",
                key: GET_ALL_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "BGM_VOL",
                key: GET_BGM_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "KOE_VOL",
                key: GET_KOE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "PCM_VOL",
                key: GET_PCM_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "SE_VOL",
                key: GET_SE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
        ],
        164 => vec![MenuItem::Int {
            label: "MSG_SPEED",
            key: GET_MESSAGE_SPEED,
            min: 0,
            max: 100,
            step: 5,
        }],
        166 => vec![
            MenuItem::Int {
                label: "AUTO_MOJI_WAIT",
                key: GET_AUTO_MODE_MOJI_WAIT,
                min: 0,
                max: 300,
                step: 5,
            },
            MenuItem::Int {
                label: "AUTO_MIN_WAIT",
                key: GET_AUTO_MODE_MIN_WAIT,
                min: 0,
                max: 10000,
                step: 100,
            },
        ],
        165 => vec![
            MenuItem::Int {
                label: "FILTER_R",
                key: GET_FILTER_COLOR_R,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_G",
                key: GET_FILTER_COLOR_G,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_B",
                key: GET_FILTER_COLOR_B,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_A",
                key: GET_FILTER_COLOR_A,
                min: 0,
                max: 255,
                step: 5,
            },
        ],
        167 => {
            ensure_font_list(syscom, project_dir);
            vec![MenuItem::FontName]
        }
        169 => vec![
            MenuItem::Int {
                label: "MOV_VOL",
                key: GET_MOV_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Bool {
                label: "MOV_ONOFF",
                key: GET_MOV_ONOFF,
            },
        ],
        168 => vec![
            MenuItem::Bool {
                label: "NO_WIPE",
                key: GET_NO_WIPE_ANIME_ONOFF,
            },
            MenuItem::Bool {
                label: "SKIP_WIPE",
                key: GET_SKIP_WIPE_ANIME_ONOFF,
            },
            MenuItem::Bool {
                label: "WHEEL_NEXT",
                key: GET_WHEEL_NEXT_MESSAGE_ONOFF,
            },
            MenuItem::Bool {
                label: "KOE_DONT_STOP",
                key: GET_KOE_DONT_STOP_ONOFF,
            },
            MenuItem::Bool {
                label: "SKIP_UNREAD",
                key: GET_SKIP_UNREAD_MESSAGE_ONOFF,
            },
            MenuItem::Bool {
                label: "PLAY_SILENT",
                key: GET_PLAY_SILENT_SOUND_ONOFF,
            },
        ],
        _ => Vec::new(),
    }
}

fn ensure_font_list(syscom: &mut globals::SyscomRuntimeState, project_dir: &Path) {
    if !syscom.font_list.is_empty() {
        return;
    }
    let dir = project_dir.join("font");
    let Ok(entries) = fs::read_dir(dir) else {
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
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                syscom.font_list.push(name.to_string());
            }
        }
    }
    syscom.font_list.sort();
}

fn build_config_menu_text(syscom: &globals::SyscomRuntimeState) -> String {
    let items = match syscom.menu_kind.unwrap_or(0) {
        158 => vec![
            MenuItem::Bool {
                label: "WINDOW_MODE",
                key: GET_WINDOW_MODE,
            },
            MenuItem::Int {
                label: "WINDOW_SIZE",
                key: GET_WINDOW_MODE_SIZE,
                min: 0,
                max: 7,
                step: 1,
            },
        ],
        159 => vec![
            MenuItem::Int {
                label: "ALL_VOL",
                key: GET_ALL_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "BGM_VOL",
                key: GET_BGM_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "KOE_VOL",
                key: GET_KOE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "PCM_VOL",
                key: GET_PCM_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "SE_VOL",
                key: GET_SE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
        ],
        164 => vec![MenuItem::Int {
            label: "MSG_SPEED",
            key: GET_MESSAGE_SPEED,
            min: 0,
            max: 100,
            step: 5,
        }],
        166 => vec![
            MenuItem::Int {
                label: "AUTO_MOJI_WAIT",
                key: GET_AUTO_MODE_MOJI_WAIT,
                min: 0,
                max: 300,
                step: 5,
            },
            MenuItem::Int {
                label: "AUTO_MIN_WAIT",
                key: GET_AUTO_MODE_MIN_WAIT,
                min: 0,
                max: 10000,
                step: 100,
            },
        ],
        165 => vec![
            MenuItem::Int {
                label: "FILTER_R",
                key: GET_FILTER_COLOR_R,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_G",
                key: GET_FILTER_COLOR_G,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_B",
                key: GET_FILTER_COLOR_B,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_A",
                key: GET_FILTER_COLOR_A,
                min: 0,
                max: 255,
                step: 5,
            },
        ],
        167 => vec![MenuItem::FontName],
        169 => vec![
            MenuItem::Int {
                label: "MOV_VOL",
                key: GET_MOV_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Bool {
                label: "MOV_ONOFF",
                key: GET_MOV_ONOFF,
            },
        ],
        168 => vec![
            MenuItem::Bool {
                label: "NO_WIPE",
                key: GET_NO_WIPE_ANIME_ONOFF,
            },
            MenuItem::Bool {
                label: "SKIP_WIPE",
                key: GET_SKIP_WIPE_ANIME_ONOFF,
            },
            MenuItem::Bool {
                label: "WHEEL_NEXT",
                key: GET_WHEEL_NEXT_MESSAGE_ONOFF,
            },
            MenuItem::Bool {
                label: "KOE_DONT_STOP",
                key: GET_KOE_DONT_STOP_ONOFF,
            },
            MenuItem::Bool {
                label: "SKIP_UNREAD",
                key: GET_SKIP_UNREAD_MESSAGE_ONOFF,
            },
            MenuItem::Bool {
                label: "PLAY_SILENT",
                key: GET_PLAY_SILENT_SOUND_ONOFF,
            },
        ],
        _ => Vec::new(),
    };
    if items.is_empty() {
        return "CONFIG MENU\n(Press Esc/Enter/Click to close)".to_string();
    }
    let mut s = String::from(
        "CONFIG MENU\nUse Up/Down + Left/Right to edit\n(Press Esc/Enter/Click to close)\n",
    );
    for (i, item) in items.iter().enumerate() {
        let cursor = if i == syscom.menu_cursor { ">" } else { " " };
        match item {
            MenuItem::Int { label, key, .. } => {
                let v = syscom.config_int.get(key).copied().unwrap_or(0);
                s.push_str(&format!("{cursor} {label}: {v}\n"));
            }
            MenuItem::Bool { label, key } => {
                let v = syscom.config_int.get(key).copied().unwrap_or(0);
                s.push_str(&format!(
                    "{cursor} {label}: {}\n",
                    if v != 0 { "ON" } else { "OFF" }
                ));
            }
            MenuItem::FontName => {
                let v = syscom
                    .config_str
                    .get(&GET_FONT_NAME)
                    .cloned()
                    .unwrap_or_default();
                s.push_str(&format!("{cursor} FONT: {v}\n"));
            }
        }
    }
    s
}
