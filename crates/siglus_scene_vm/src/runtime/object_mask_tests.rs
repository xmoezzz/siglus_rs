use super::*;

struct Fixture {
    ctx: CommandContext,
    directory: PathBuf,
    layer: LayerId,
    masks: [ImageHandle; 2],
    next_slot: usize,
}

impl Fixture {
    fn new() -> Self {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "siglus-object-mask-{}-{}-{serial}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir(&directory).unwrap();
        for index in 0..2 {
            let mut image = image::RgbaImage::from_pixel(3, 3, image::Rgba([255, 255, 255, 0]));
            image.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
            image
                .save(directory.join(format!("mask{index}.png")))
                .unwrap();
        }
        let mut ctx = CommandContext::new(directory.clone());
        ctx.set_screen_size(100, 100);
        let layer = ctx.layers.create_layer();
        let masks = std::array::from_fn(|index| {
            ctx.images
                .load_file(&directory.join(format!("mask{index}.png")), 0)
                .unwrap()
        });
        let mut list = globals::MaskListState::new(2);
        for (index, mask) in list.masks.iter_mut().enumerate() {
            mask.name = Some(format!("mask{index}.png"));
            mask.x_event.set_value(10 + index as i32 * 30);
            mask.x_event.frame();
            mask.y_event.set_value(20 + index as i32 * 30);
            mask.y_event.frame();
        }
        ctx.globals
            .mask_lists
            .insert(ctx.ids.form_global_mask, list);
        Self {
            ctx,
            directory,
            layer,
            masks,
            next_slot: 100_000,
        }
    }

    fn container(&mut self, mask: i64) -> globals::ObjectState {
        let mut object = globals::ObjectState::default();
        object.init_param_like();
        object.used = true;
        object.set_int_prop(&self.ctx.ids, self.ctx.ids.obj_disp, 1);
        object.set_int_prop(&self.ctx.ids, self.ctx.ids.obj_mask_no, mask);
        object.nested_runtime_slot = Some(self.next_slot);
        self.next_slot += 1;
        object
    }

    fn leaf(&mut self, mask: i64) -> (globals::ObjectState, SpriteId) {
        let image = self.ctx.images.solid_rgba((255, 128, 0, 255));
        let layer = self.ctx.layers.layer_mut(self.layer).unwrap();
        let sprite_id = layer.create_sprite();
        let sprite = layer.sprite_mut(sprite_id).unwrap();
        sprite.image_id = Some(image);
        sprite.size_mode = SpriteSizeMode::Explicit {
            width: 20,
            height: 20,
        };
        let mut object = self.container(mask);
        object.object_type = 1;
        object.backend = globals::ObjectBackend::Rect {
            layer_id: self.layer,
            sprite_id,
            width: 20,
            height: 20,
        };
        object.rect_param.right = 20;
        object.rect_param.bottom = 20;
        (object, sprite_id)
    }

    fn install(&mut self, mut roots: Vec<globals::ObjectState>) {
        for object in &mut roots {
            object.nested_runtime_slot = None;
        }
        self.ctx
            .globals
            .stage_forms
            .entry(self.ctx.ids.form_global_stage)
            .or_default()
            .object_lists
            .insert(TNM_STAGE_FRONT_I64, roots);
    }

    fn root_mut(&mut self) -> &mut globals::ObjectState {
        &mut self
            .ctx
            .globals
            .stage_forms
            .get_mut(&self.ctx.ids.form_global_stage)
            .unwrap()
            .object_lists
            .get_mut(&TNM_STAGE_FRONT_I64)
            .unwrap()[0]
    }

    fn render(&mut self) -> Vec<RenderSprite> {
        self.ctx.render_frame_with_effects().sprites
    }

    fn assert_mask(
        &self,
        sprites: &[RenderSprite],
        id: SpriteId,
        expected: Option<(usize, i32, i32)>,
    ) {
        let sprite = &sprites
            .iter()
            .find(|entry| entry.layer_id == Some(self.layer) && entry.sprite_id == Some(id))
            .expect("fixture leaf must be rendered")
            .sprite;
        assert_eq!(
            sprite.mask_image_id.as_ref().map(|mask| mask.key()),
            expected.map(|(index, _, _)| self.masks[index].key())
        );
        assert_eq!(
            (sprite.mask_offset_x, sprite.mask_offset_y),
            expected.map(|(_, x, y)| (x, y)).unwrap_or((0, 0))
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for index in 0..2 {
            let _ = std::fs::remove_file(self.directory.join(format!("mask{index}.png")));
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[test]
fn sprite_less_container_masks_nested_children_without_masking_other_roots() {
    let mut fixture = Fixture::new();
    let mut root = fixture.container(0);
    let mut nested = fixture.container(-1);
    // Mask offsets are in screen space, independent of object transforms.
    root.set_int_prop(&fixture.ctx.ids, fixture.ctx.ids.obj_x, 17);
    nested.set_int_prop(&fixture.ctx.ids, fixture.ctx.ids.obj_y, 9);
    let (first, first_id) = fixture.leaf(-1);
    let (second, second_id) = fixture.leaf(-1);
    let (outside, outside_id) = fixture.leaf(-1);
    nested.runtime.child_objects.push(first);
    root.runtime.child_objects = vec![nested, second];
    fixture.install(vec![root, outside]);
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, first_id, Some((0, 10, 20)));
    fixture.assert_mask(&sprites, second_id, Some((0, 10, 20)));
    fixture.assert_mask(&sprites, outside_id, None);
}

#[test]
fn only_default_mask_numbers_inherit_and_explicit_missing_masks_block() {
    let mut fixture = Fixture::new();
    let mut root = fixture.container(0);
    let mut override_group = fixture.container(1);
    let (leaf, leaf_id) = fixture.leaf(-1);
    override_group.runtime.child_objects.push(leaf);
    let (own, own_id) = fixture.leaf(1);
    let (missing, missing_id) = fixture.leaf(999);
    let (negative, negative_id) = fixture.leaf(-2);
    let mut missing_group = fixture.container(999);
    let (nested, nested_id) = fixture.leaf(-1);
    missing_group.runtime.child_objects.push(nested);
    let (sibling, sibling_id) = fixture.leaf(-1);
    root.runtime.child_objects = vec![
        override_group,
        own,
        missing,
        negative,
        missing_group,
        sibling,
    ];
    fixture.install(vec![root]);
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, leaf_id, Some((1, 40, 50)));
    fixture.assert_mask(&sprites, own_id, Some((1, 40, 50)));
    fixture.assert_mask(&sprites, missing_id, None);
    fixture.assert_mask(&sprites, negative_id, None);
    fixture.assert_mask(&sprites, nested_id, None);
    fixture.assert_mask(&sprites, sibling_id, Some((0, 10, 20)));

    // A valid slot without an image is also explicit: C++ resolves no texture
    // after choosing that slot, rather than falling back to the parent mask.
    fixture
        .ctx
        .globals
        .mask_lists
        .get_mut(&fixture.ctx.ids.form_global_mask)
        .unwrap()
        .masks[1]
        .name = None;
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, leaf_id, None);
    fixture.assert_mask(&sprites, own_id, None);
    fixture.assert_mask(&sprites, sibling_id, Some((0, 10, 20)));
}

#[test]
fn inherited_mask_moves_switches_and_clears_without_changing_saved_properties() {
    let mut fixture = Fixture::new();
    let mut root = fixture.container(0);
    let (leaf, id) = fixture.leaf(-1);
    root.runtime.child_objects.push(leaf);
    fixture.install(vec![root]);
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, id, Some((0, 10, 20)));
    let list = fixture
        .ctx
        .globals
        .mask_lists
        .get_mut(&fixture.ctx.ids.form_global_mask)
        .unwrap();
    list.masks[0].x_event.set_value(-35);
    list.masks[0].x_event.frame();
    list.masks[0].y_event.set_value(70);
    list.masks[0].y_event.frame();
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, id, Some((0, -35, 70)));
    let ids = fixture.ctx.ids.clone();
    fixture.root_mut().set_int_prop(&ids, ids.obj_mask_no, 1);
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, id, Some((1, 40, 50)));
    assert_eq!(
        fixture.root_mut().runtime.child_objects[0].lookup_int_prop(&ids, ids.obj_mask_no),
        Some(-1)
    );
    fixture.root_mut().set_int_prop(&ids, ids.obj_mask_no, -1);
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, id, None);
}

#[test]
fn drawable_parents_and_explicit_leaf_masks_keep_existing_behavior() {
    let mut fixture = Fixture::new();
    let (mut root, root_id) = fixture.leaf(0);
    let (child, child_id) = fixture.leaf(-1);
    let (own, own_id) = fixture.leaf(1);
    let (missing, missing_id) = fixture.leaf(999);
    let mut missing_group = fixture.container(999);
    let (nested, nested_id) = fixture.leaf(-1);
    missing_group.runtime.child_objects.push(nested);
    root.runtime.child_objects = vec![child, own, missing, missing_group];
    fixture.install(vec![root]);
    let sprites = fixture.render();
    fixture.assert_mask(&sprites, root_id, Some((0, 10, 20)));
    fixture.assert_mask(&sprites, child_id, Some((0, 10, 20)));
    fixture.assert_mask(&sprites, own_id, Some((1, 40, 50)));
    fixture.assert_mask(&sprites, missing_id, None);
    fixture.assert_mask(&sprites, nested_id, None);
}
