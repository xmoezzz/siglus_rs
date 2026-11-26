use super::{
    CommandContext, ImageManager, LayerManager, ParentRenderState, Sprite, SpriteFit,
    SpriteSizeMode, button_event_enabled, button_parent_render_state, constants, globals, graphics,
    has_standalone_button_registration, hit_test_render_sprite, input::VmKey, input::VmMouseButton,
    object_button_bound_hit_sprites, object_button_fallback_sprite,
    object_button_renderable_by_syscom, object_runtime_slot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StageButton {
    form: u32,
    stage: i64,
    group: Option<usize>,
    slot: usize,
    number: i64,
}

#[derive(Clone, Copy, Debug)]
struct Target {
    button: StageButton,
    point: (i32, i32),
}

impl CommandContext {
    pub(super) fn has_active_mwnd_selection(&self) -> bool {
        self.globals.focused_stage_mwnd.is_some()
            || self.globals.stage_forms.values().any(|stage| {
                stage
                    .mwnd_lists
                    .values()
                    .flatten()
                    .any(|m| m.open && m.selection.is_some())
            })
    }

    pub(super) fn handle_stage_button_navigation(&mut self, key: VmKey) -> bool {
        let direction = match key {
            VmKey::ArrowLeft => (-1, 0),
            VmKey::ArrowRight => (1, 0),
            VmKey::ArrowUp => (0, -1),
            VmKey::ArrowDown => (0, 1),
            VmKey::Enter if self.stage_button_focus.is_some() => (0, 0),
            _ => return false,
        };
        if self.input.is_mouse_down(VmMouseButton::Left) {
            return false;
        }

        let targets = self.stage_button_targets();
        let focused = targets
            .iter()
            .find(|target| self.stage_button_focus == Some(target.button));
        if key == VmKey::Enter {
            self.stage_button_focus = None;
            let Some(target) = focused else {
                return false;
            };
            self.on_mouse_move(target.point.0, target.point.1);
            if self.stage_button_has_hit(target.button) {
                self.on_mouse_down(VmMouseButton::Left);
                self.on_mouse_up(VmMouseButton::Left);
            }
            return true;
        }

        let current = focused.or_else(|| {
            targets
                .iter()
                .find(|target| self.stage_button_has_hit(target.button))
        });
        let next = if let Some(current) = current {
            targets
                .iter()
                .filter_map(|target| {
                    direction_score(current.point, target.point, direction)
                        .map(|score| (score, target))
                })
                .min_by_key(|(score, _)| *score)
                .map(|(_, target)| target)
                .or(Some(current))
        } else {
            targets
                .iter()
                .min_by_key(|target| (target.button.number, target.point.1, target.point.0))
        };
        let Some(next) = next else {
            self.stage_button_focus = None;
            return false;
        };
        self.on_mouse_move(next.point.0, next.point.1);
        self.stage_button_focus = Some(next.button);
        true
    }

    fn stage_button_targets(&mut self) -> Vec<Target> {
        let Some(form_id) = self.active_button_stage_form_id() else {
            return Vec::new();
        };
        let Some(stage_form) = self.globals.stage_forms.get(&form_id) else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        let mut stages: Vec<_> = stage_form.object_lists.keys().copied().collect();
        stages.sort_unstable();
        let read_skip = self.runtime_read_skip_is_enable();
        let mut collector = TargetCollector {
            images: &mut self.images,
            layers: &self.layers,
            gfx: &self.gfx,
            ids: &self.ids,
            syscom: &self.globals.syscom,
            read_skip,
            screen: (self.screen_w, self.screen_h),
            form: form_id,
            targets: &mut targets,
        };
        for stage in stages {
            let groups = stage_form
                .group_lists
                .get(&stage)
                .map_or(&[][..], Vec::as_slice);
            let focus_group = self.globals.focused_stage_group;
            if focus_group
                .is_some_and(|(form, focused_stage, _)| form != form_id || focused_stage != stage)
            {
                continue;
            }
            let focused = focus_group.map(|(_, _, group)| group);
            let embedded = stage_form.embedded_object_slots_by_stage.get(&stage);
            let used = stage_form.object_slot_use.get(&stage);
            for (index, object) in stage_form.object_lists[&stage].iter().enumerate() {
                if !embedded.is_some_and(|slots| slots.contains(&index))
                    && used
                        .and_then(|flags| flags.get(index))
                        .copied()
                        .unwrap_or(true)
                {
                    collector.collect(stage, groups, focused, index, object, None, None);
                }
            }
        }
        targets
    }

    fn stage_button_has_hit(&self, button: StageButton) -> bool {
        let Some(form) = self.globals.stage_forms.get(&button.form) else {
            return false;
        };
        if let Some(group) = button.group {
            return form
                .group_lists
                .get(&button.stage)
                .and_then(|groups| groups.get(group))
                .is_some_and(|group| {
                    group.is_doing() && group.hit_runtime_slot == Some(button.slot)
                });
        }
        fn hit(objects: &[globals::ObjectState], slot: usize) -> bool {
            objects.iter().enumerate().any(|(index, object)| {
                (object_runtime_slot(index, object) == slot && object.button.hit)
                    || hit(&object.runtime.child_objects, slot)
            })
        }
        form.object_lists
            .get(&button.stage)
            .is_some_and(|objects| hit(objects, button.slot))
    }
}

struct TargetCollector<'a> {
    images: &'a mut ImageManager,
    layers: &'a LayerManager,
    gfx: &'a graphics::GfxRuntime,
    ids: &'a constants::RuntimeConstants,
    syscom: &'a globals::SyscomRuntimeState,
    read_skip: bool,
    screen: (u32, u32),
    form: u32,
    targets: &'a mut Vec<Target>,
}

impl TargetCollector<'_> {
    fn collect(
        &mut self,
        stage: i64,
        groups: &[globals::GroupState],
        focused: Option<usize>,
        index: usize,
        object: &globals::ObjectState,
        parent: Option<ParentRenderState>,
        inherited: Option<StageButton>,
    ) {
        let slot = object_runtime_slot(index, object);
        let group = object.button.group_idx();
        let owns_button = !object.base.no_event_hint
            && object.button.action_no >= 0
            && match group {
                Some(group) => {
                    focused.is_none_or(|focus| focus == group)
                        && groups.get(group).is_some_and(globals::GroupState::is_doing)
                }
                None => focused.is_none() && has_standalone_button_registration(object),
            };
        let button = if owns_button {
            button_event_enabled(self.syscom, object, None, self.read_skip).then_some(StageButton {
                form: self.form,
                stage,
                group,
                slot,
                number: object.button.button_no,
            })
        } else {
            inherited
        };
        if let Some(button) = button
            && !object.base.no_event_hint
            && object_button_renderable_by_syscom(self.syscom, object)
            && !self.targets.iter().any(|target| target.button == button)
        {
            let bound = object_button_bound_hit_sprites(
                self.layers,
                self.gfx,
                self.ids,
                stage,
                index,
                object,
                &parent,
            );
            let point = if bound.is_empty() {
                object_button_fallback_sprite(
                    self.images,
                    self.layers,
                    self.gfx,
                    self.ids,
                    stage,
                    slot,
                    object,
                    parent.clone(),
                )
                .and_then(|sprite| self.target_point(&sprite, object.button.alpha_test))
            } else {
                bound
                    .iter()
                    .find_map(|rs| self.target_point(&rs.sprite, object.button.alpha_test))
            };
            if let Some(point) = point {
                self.targets.push(Target { button, point });
            }
        }
        if !object.runtime.child_objects.is_empty() {
            let parent = button_parent_render_state(
                self.layers,
                self.gfx,
                self.ids,
                stage,
                index,
                object,
                parent,
            );
            for (index, child) in object.runtime.child_objects.iter().enumerate() {
                self.collect(
                    stage,
                    groups,
                    focused,
                    index,
                    child,
                    Some(parent.clone()),
                    button,
                );
            }
        }
    }

    fn target_point(&mut self, sprite: &Sprite, alpha_test: bool) -> Option<(i32, i32)> {
        if !sprite.visible || sprite.tr == 0 || sprite.scale_x == 0.0 || sprite.scale_y == 0.0 {
            return None;
        }
        let (w, h) = match sprite.size_mode {
            SpriteSizeMode::Explicit { width, height } => (width, height),
            SpriteSizeMode::Intrinsic => sprite
                .image_id
                .as_ref()
                .and_then(|id| self.images.get(id))
                .map(|img| (img.width, img.height))
                .or_else(|| {
                    sprite
                        .emote_render
                        .as_ref()
                        .map(|img| (img.width, img.height))
                })?,
        };
        let (anchor_x, anchor_y) = match sprite.fit {
            SpriteFit::PixelRect => (sprite.x as f32, sprite.y as f32),
            SpriteFit::FullScreen => (0.0, 0.0),
        };
        let (origin_x, origin_y, center_x, center_y) = if sprite.object_anchor {
            (
                anchor_x,
                anchor_y,
                sprite.texture_center_x,
                sprite.texture_center_y,
            )
        } else {
            (
                anchor_x + sprite.pivot_x,
                anchor_y + sprite.pivot_y,
                0.0,
                0.0,
            )
        };
        let (sin, cos) = sprite.rotate.sin_cos();
        let samples = [
            (0.5, 0.5),
            (0.25, 0.25),
            (0.75, 0.25),
            (0.25, 0.75),
            (0.75, 0.75),
        ];
        let mut try_uv = |u: f32, v: f32| {
            let x = (w as f32 * u - sprite.pivot_x - center_x) * sprite.scale_x;
            let y = (h as f32 * v - sprite.pivot_y - center_y) * sprite.scale_y;
            let x_pos = (origin_x + x * cos - y * sin).round() as i32;
            let y_pos = (origin_y + x * sin + y * cos).round() as i32;
            (x_pos >= 0
                && y_pos >= 0
                && (x_pos as u32) < self.screen.0
                && (y_pos as u32) < self.screen.1
                && hit_test_render_sprite(self.images, sprite, x_pos, y_pos, alpha_test))
            .then_some((x_pos, y_pos))
        };
        if let Some(point) = samples.into_iter().find_map(|(u, v)| try_uv(u, v)) {
            return Some(point);
        }
        // Fallback grid for alpha-tested hollow/irregular shapes or dst_clip.
        const GRID_STEPS: usize = 8;
        for gy in 1..GRID_STEPS {
            let v = gy as f32 / GRID_STEPS as f32;
            for gx in 1..GRID_STEPS {
                let u = gx as f32 / GRID_STEPS as f32;
                if let Some(point) = try_uv(u, v) {
                    return Some(point);
                }
            }
        }
        None
    }
}

fn direction_score(from: (i32, i32), to: (i32, i32), direction: (i64, i64)) -> Option<i64> {
    let dx = i64::from(to.0) - i64::from(from.0);
    let dy = i64::from(to.1) - i64::from(from.1);
    let forward = dx * direction.0 + dy * direction.1;
    let cross = (dx * direction.1 - dy * direction.0).abs();
    (forward > 0 && cross <= 2 * forward).then_some(forward + 2 * cross)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::RgbaImage;
    use crate::layer::ClipRect;

    #[test]
    fn target_point_falls_back_to_grid_for_hollow_alpha_test_button() {
        let mut ctx = CommandContext::new(std::path::PathBuf::from("."));
        ctx.screen_w = 640;
        ctx.screen_h = 480;

        // 80x80 ring: only the outer 12px border is opaque; all 5 fast sample
        // points (0.5,0.5) and (0.25/0.75) are transparent.
        let mut rgba = vec![0u8; 80 * 80 * 4];
        for y in 0..80 {
            for x in 0..80 {
                if !(12..68).contains(&x) || !(12..68).contains(&y) {
                    let idx = (y * 80 + x) * 4;
                    rgba[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        let img = ctx.images.insert_image(RgbaImage {
            width: 80,
            height: 80,
            center_x: 0,
            center_y: 0,
            rgba,
        });
        let sprite = Sprite {
            visible: true,
            tr: 255,
            x: 100,
            y: 100,
            image_id: Some(img),
            ..Default::default()
        };

        let mut targets = Vec::new();
        let mut collector = TargetCollector {
            images: &mut ctx.images,
            layers: &ctx.layers,
            gfx: &ctx.gfx,
            ids: &ctx.ids,
            syscom: &ctx.globals.syscom,
            read_skip: false,
            screen: (ctx.screen_w, ctx.screen_h),
            form: 0,
            targets: &mut targets,
        };
        let point = collector
            .target_point(&sprite, true)
            .expect("grid fallback should find opaque border");
        assert!(hit_test_render_sprite(
            &mut ctx.images,
            &sprite,
            point.0,
            point.1,
            true
        ));
    }

    #[test]
    fn target_point_respects_dst_clip_via_grid_fallback() {
        let mut ctx = CommandContext::new(std::path::PathBuf::from("."));
        ctx.screen_w = 640;
        ctx.screen_h = 480;

        let rgba = vec![255u8; 80 * 80 * 4];
        let img = ctx.images.insert_image(RgbaImage {
            width: 80,
            height: 80,
            center_x: 0,
            center_y: 0,
            rgba,
        });
        let sprite = Sprite {
            visible: true,
            tr: 255,
            x: 100,
            y: 100,
            image_id: Some(img),
            // Clip excludes all 5 fast sample points (which are at x/y = 120, 140, 160).
            dst_clip: Some(ClipRect {
                left: 100,
                top: 100,
                right: 118,
                bottom: 118,
            }),
            ..Default::default()
        };

        let mut targets = Vec::new();
        let mut collector = TargetCollector {
            images: &mut ctx.images,
            layers: &ctx.layers,
            gfx: &ctx.gfx,
            ids: &ctx.ids,
            syscom: &ctx.globals.syscom,
            read_skip: false,
            screen: (ctx.screen_w, ctx.screen_h),
            form: 0,
            targets: &mut targets,
        };
        let point = collector
            .target_point(&sprite, false)
            .expect("grid fallback should find point inside dst_clip");
        assert!(point.0 >= 100 && point.0 < 118);
        assert!(point.1 >= 100 && point.1 < 118);
        assert!(hit_test_render_sprite(
            &mut ctx.images,
            &sprite,
            point.0,
            point.1,
            false
        ));
    }
}
