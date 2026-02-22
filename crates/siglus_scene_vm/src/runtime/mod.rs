//! Runtime scaffolding for command execution.
//!
//! This layer is intentionally pragmatic:
//! - It supports named commands used by standalone bring-up tools.
//! - It supports numeric dispatch (forms/syscalls) used by the VM.
//! - Unknown or unfinished operations are recorded instead of crashing.

pub mod commands;
pub mod graphics;
pub mod forms;
pub mod input;
pub mod id_map;
pub mod opcode;

pub use opcode::OpCode;
pub mod syscalls;
pub mod tables;
pub mod unknown;
pub mod wait;
pub mod ui;
pub mod globals;
pub mod int_event;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::{AudioHub, BgmEngine, PcmEngine, SeEngine};
use crate::image_manager::ImageManager;
use crate::layer::LayerManager;
use crate::movie::MovieManager;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Str(String),
    /// An element chain as raw i32 codes (as stored on the VM int stack).
    Element(Vec<i32>),
    /// A nested list value (FM_LIST).
    List(Vec<Value>),
    /// A named argument (id -> value), used by some engine commands.
    NamedArg { id: i32, value: Box<Value> },
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
    /// Optional numeric code for VM forms/syscalls.
    pub code: Option<opcode::OpCode>,
    pub args: Vec<Value>,
}

/// State used by a subset of syscalls (mostly EXCALL).
///
/// We intentionally keep these names offset-based instead of guessing their meaning.
#[derive(Debug, Default, Clone)]
pub struct SyscallState {
    pub flag_204: bool,
    pub flag_2148: bool,
}


/// Optional external handler for numeric forms.
///
/// The project can keep game-specific implementations (e.g. SCREEN/MSGBK)
/// outside this crate, while still letting the VM dispatch through here.
pub trait ExternalFormHandler: Send + Sync {
    /// Return true if the form ID was handled.
    fn dispatch_form(&self, ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> anyhow::Result<bool>;
}

/// Optional external handler for numeric syscalls.
pub trait ExternalSyscallHandler: Send + Sync {
    /// Return true if the syscall ID was handled.
    fn dispatch_syscall(&self, ctx: &mut CommandContext, syscall_id: u32, args: &[Value]) -> anyhow::Result<bool>;
}

pub struct CommandContext {
    pub project_dir: PathBuf,

    pub images: ImageManager,
    pub layers: LayerManager,

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

    /// VM-visible input state (queried via INPUT/MOUSE/KEYLIST forms).
    pub input: input::InputState,

    /// Current render target size (used for UI layout).
    pub screen_w: u32,
    pub screen_h: u32,

    /// VM blocking state (WAIT / WAIT_KEY).
    pub wait: wait::VmWait,

    /// Gameexe-driven asset tables (CGTABLE / DATABASE / THUMBTABLE).
    pub tables: tables::AssetTables,

    /// Value stack used by form/syscall handlers to return results.
    pub stack: Vec<Value>,

    pub unknown: unknown::UnknownOpRecorder,

    pub globals: globals::GlobalState,

    pub syscalls: SyscallState,

    /// Optional project-provided form handler (game-specific).
    pub external_forms: Option<Arc<dyn ExternalFormHandler>>,

    /// Optional project-provided syscall handler (game-specific).
    pub external_syscalls: Option<Arc<dyn ExternalSyscallHandler>>,
}

impl CommandContext {
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

        Self {
            images: ImageManager::new(project_dir.clone()),
            layers: LayerManager::default(),
            audio,
			bgm: BgmEngine::new(project_dir.clone()),
			pcm: PcmEngine::new(project_dir.clone()),
			se: SeEngine::new(project_dir.clone()),
            movie: MovieManager::new(project_dir.clone()),
            project_dir,
            tables,
            stack: Vec::new(),
            unknown,
            ids,
            gfx: graphics::GfxRuntime::default(),
            ui: ui::UiRuntime::default(),
            input: input::InputState::default(),
            wait: wait::VmWait::default(),

            screen_w: 1280,
            screen_h: 720,
            globals: globals::GlobalState::default(),
            syscalls: SyscallState::default(),

            external_forms: None,
            external_syscalls: None,
        }
    }


    /// Install or clear an external form handler.
    pub fn set_external_form_handler(&mut self, h: Option<Arc<dyn ExternalFormHandler>>) {
        self.external_forms = h;
    }

    /// Install or clear an external syscall handler.
    pub fn set_external_syscall_handler(&mut self, h: Option<Arc<dyn ExternalSyscallHandler>>) {
        self.external_syscalls = h;
    }



    // ------------------------------------------------------------------
    // Object button bring-up
    // ------------------------------------------------------------------

    fn load_any_image_for_hit(images: &mut ImageManager, file: &str, patno: i64) -> Option<crate::image_manager::ImageId> {
        let pat_u32 = if patno < 0 { 0 } else { patno as u32 };
        if let Ok(id) = images.load_g00(file, pat_u32) {
            return Some(id);
        }
        if let Ok(id) = images.load_bg(file) {
            return Some(id);
        }
        None
    }

    fn hit_test_sprite_rect(
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        mx: i32,
        my: i32,
    ) -> bool {
        let x2 = x.saturating_add(w as i32);
        let y2 = y.saturating_add(h as i32);
        mx >= x && mx < x2 && my >= y && my < y2
    }

    fn alpha_test_image(
        img: &crate::assets::RgbaImage,
        local_x: i32,
        local_y: i32,
    ) -> bool {
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
                        globals::ObjectBackend::Rect { layer_id, sprite_id, .. } => self
                            .layers
                            .layer(layer_id)
                            .and_then(|l| l.sprite(sprite_id))
                            .map(|spr| spr.visible)
                            .unwrap_or(false),
                        globals::ObjectBackend::Gfx => self.gfx.object_peek_disp(*stage_idx, obj_i as i64).unwrap_or(0) != 0,
                        _ => false,
                    };
                    if !visible {
                        continue;
                    }

                    // Bounding box + optional alpha test.
                    let mut hit = false;
                    match obj.backend {
                        globals::ObjectBackend::Rect { layer_id, sprite_id, width, height } => {
                            if let Some(spr) = self.layers.layer(layer_id).and_then(|l| l.sprite(sprite_id)) {
                                hit = Self::hit_test_sprite_rect(spr.x, spr.y, width, height, mx, my);
                                if hit && obj.button.alpha_test {
                                    if let Some(img_id) = spr.image_id {
                                        if let Some(img) = self.images.get(img_id).map(|a| a.as_ref()) {
                                            hit = Self::alpha_test_image(img, mx - spr.x, my - spr.y);
                                        }
                                    }
                                }
                            }
                        }
                        globals::ObjectBackend::Gfx => {
                            let (x, y) = self.gfx.object_peek_pos(*stage_idx, obj_i as i64).unwrap_or((0, 0));
                            let patno = self.gfx.object_peek_patno(*stage_idx, obj_i as i64).unwrap_or(0);
                            if let Some(file) = obj.file_name.as_deref() {
                                if let Some(img_id) = Self::load_any_image_for_hit(&mut self.images, file, patno) {
                                    if let Some(img) = self.images.get(img_id).map(|a| a.as_ref()) {
                                        hit = Self::hit_test_sprite_rect(x as i32, y as i32, img.width, img.height, mx, my);
                                        if hit && obj.button.alpha_test {
                                            hit = Self::alpha_test_image(img, mx - x as i32, my - y as i32);
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

                    // Draw order (best-effort): use sprite order if present, else layer/order.
                    let draw_order = match obj.backend {
                        globals::ObjectBackend::Rect { layer_id, sprite_id, .. } => self
                            .layers
                            .layer(layer_id)
                            .and_then(|l| l.sprite(sprite_id))
                            .map(|spr| spr.order as i64)
                            .unwrap_or(0),
                        globals::ObjectBackend::Gfx => {
                            let layer_no = self.gfx.object_peek_layer(*stage_idx, obj_i as i64).unwrap_or(0);
                            let order = self.gfx.object_peek_order(*stage_idx, obj_i as i64).unwrap_or(0);
                            layer_no.saturating_mul(1000).saturating_add(order)
                        }
                        _ => 0,
                    };

                    let btn_no = obj.button.button_no;
                    match best {
                        None => best = Some((btn_no, draw_order, obj_i)),
                        Some((_b, bo, _oi)) if draw_order > bo => best = Some((btn_no, draw_order, obj_i)),
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
            // Best-effort: clear pushed flag unless a script keeps it.
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
        self.input.on_key_down(k);

        // EditBox bring-up: when an editbox is focused, map Enter/Esc to decided/canceled
        // to prevent script wait-loops from deadlocking.
        if let Some((form_id, idx)) = self.globals.focused_editbox {
            if let Some(list) = self.globals.editbox_lists.get_mut(&form_id) {
                if let Some(eb) = list.boxes.get_mut(idx) {
                    match k {
                        input::VmKey::Enter => {
                            eb.decided = true;
                        }
                        input::VmKey::Escape => {
                            eb.canceled = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        
        // Stage group selection bring-up: map Enter/Escape to a decision.
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

        if self.ui.waiting_message {
            self.ui.end_wait_message();
        }
        let wipe_skipped = self.wait.notify_key();
        while let Some(info) = self.wait.take_movie_skip() {
            if let Some(st) = self.globals.stage_forms.get_mut(&info.stage_form_id) {
                if let Some(list) = st.object_lists.get_mut(&info.stage_idx) {
                    if info.obj_idx < list.len() {
                        // key skip triggers C_elm_object::init_type(true).
                        list[info.obj_idx].init_type_like();
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

    pub fn on_mouse_move(&mut self, x: i32, y: i32) {
        self.input.on_mouse_move(x, y);
        self.update_object_button_hover();
    }

    pub fn on_mouse_down(&mut self, b: input::VmMouseButton) {
        self.input.on_mouse_down(b);
        self.handle_object_button_mouse_down(b);
        if self.ui.waiting_message {
            self.ui.end_wait_message();
        }
        let wipe_skipped = self.wait.notify_key();
        while let Some(info) = self.wait.take_movie_skip() {
            if let Some(st) = self.globals.stage_forms.get_mut(&info.stage_form_id) {
                if let Some(list) = st.object_lists.get_mut(&info.stage_idx) {
                    if info.obj_idx < list.len() {
                        // key skip triggers C_elm_object::init_type(true).
                        list[info.obj_idx].init_type_like();
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
        if self.ui.waiting_message {
            self.ui.end_wait_message();
        }
        let wipe_skipped = self.wait.notify_key();
        while let Some(info) = self.wait.take_movie_skip() {
            if let Some(st) = self.globals.stage_forms.get_mut(&info.stage_form_id) {
                if let Some(list) = st.object_lists.get_mut(&info.stage_idx) {
                    if info.obj_idx < list.len() {
                        // key skip triggers C_elm_object::init_type(true).
                        list[info.obj_idx].init_type_like();
                    }
                }
            }
        }
        if wipe_skipped {
            self.globals.finish_wipe();
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
        self.ui.tick(&mut self.layers, self.screen_w, self.screen_h);
        self.globals.tick_frame();
    }
}

pub fn dispatch(ctx: &mut CommandContext, cmd: &Command) -> Result<()> {
    // Numeric dispatch first (forms/syscalls). If we don't recognize it yet,
    // we record it and keep going.
    if let Some(code) = cmd.code {
        if opcode::dispatch_code(ctx, code, &cmd.args)? {
            return Ok(());
        }
        ctx.unknown.record_code(code);
        return Ok(());
    }

    // Named command dispatch (bring-up tools).
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
