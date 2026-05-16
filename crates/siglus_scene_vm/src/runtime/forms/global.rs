use anyhow::Result;

use std::path::{Path, PathBuf};

use crate::runtime::forms::codes::int_event_op;
use crate::runtime::globals::WipeState;
use crate::runtime::{constants, forms, CommandContext, Value};

use crate::runtime::forms::{
    cgtable, counter, database, editbox, file, frame_action, frame_action_ch, g00buf, input,
    int_event, int_list, key, keylist, mask, math, mouse, object_event, script, stage, steam,
    str_list, syscom, system, timewait,
};

fn canonical_global_form_id(ctx: &CommandContext, form_id: u32) -> u32 {
    let ids = &ctx.ids;
    if constants::is_stage_global_form(form_id, ids.form_global_stage) {
        return constants::global_form::STAGE_ALT;
    }
    if constants::matches_form_id(form_id, ids.form_global_mov, constants::global_form::MOV) {
        return constants::global_form::MOV;
    }
    if constants::matches_form_id(form_id, ids.form_global_bgm, constants::global_form::BGM) {
        return constants::global_form::BGM;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_bgm_table,
        constants::global_form::BGMTABLE,
    ) {
        return constants::global_form::BGMTABLE;
    }
    if constants::matches_form_id(form_id, ids.form_global_pcm, constants::global_form::PCM) {
        return constants::global_form::PCM;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_pcmch,
        constants::global_form::PCMCH,
    ) {
        return constants::global_form::PCMCH;
    }
    if constants::matches_form_id(form_id, ids.form_global_se, constants::global_form::SE) {
        return constants::global_form::SE;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_pcm_event,
        constants::global_form::PCMEVENT,
    ) {
        return constants::global_form::PCMEVENT;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_excall,
        constants::global_form::EXCALL,
    ) {
        return constants::global_form::EXCALL;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_screen,
        constants::global_form::SCREEN,
    ) {
        return constants::global_form::SCREEN;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_msgbk,
        constants::global_form::MSGBK,
    ) {
        return constants::global_form::MSGBK;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_koe_st,
        constants::global_form::KOE_ST,
    ) {
        return constants::global_form::KOE_ST;
    }
    if constants::matches_form_id(form_id, ids.form_global_key, constants::global_form::KEY) {
        return constants::global_form::KEY;
    }
    if constants::matches_form_id(
        form_id,
        ids.form_global_frame_action,
        constants::global_form::FRAME_ACTION,
    ) {
        return constants::global_form::FRAME_ACTION;
    }
    if form_id == constants::global_form::TIMEWAIT {
        return constants::global_form::TIMEWAIT;
    }
    if form_id == constants::global_form::TIMEWAIT_KEY {
        return constants::global_form::TIMEWAIT_KEY;
    }
    if form_id == constants::global_form::COUNTER {
        return constants::global_form::COUNTER;
    }
    form_id
}

fn named_i64(args: &[Value], id: i32) -> Option<i64> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: got, value } if *got == id => value.as_i64(),
        _ => None,
    })
}

fn positional_i64(args: &[Value], idx: usize) -> Option<i64> {
    args.iter()
        .filter(|v| !matches!(v, Value::NamedArg { .. }))
        .filter_map(Value::as_i64)
        .nth(idx)
}

fn global_stage_alias_to_index(form_id: i32) -> Option<i64> {
    let form_id = form_id as u32;
    if form_id == constants::global_form::BACK {
        Some(0)
    } else if form_id == constants::global_form::FRONT {
        Some(1)
    } else if form_id == constants::global_form::NEXT {
        Some(2)
    } else {
        None
    }
}

fn mwnd_ref_from_value(v: &Value) -> Option<(i64, usize)> {
    match v.unwrap_named() {
        Value::Int(n) if *n >= 0 => Some((1, *n as usize)),
        Value::Element(chain) => {
            let stage = chain
                .first()
                .and_then(|head| global_stage_alias_to_index(*head))
                .unwrap_or(1);
            let no = chain.windows(2).find_map(|w| {
                (w[0] == forms::codes::ELM_ARRAY && w[1] >= 0).then_some(w[1] as usize)
            })?;
            Some((stage, no))
        }
        _ => None,
    }
}

fn mwnd_no_from_value(v: &Value) -> Option<usize> {
    mwnd_ref_from_value(v).map(|(_, no)| no)
}

const TNM_STAGE_FRONT_SELBTN: i64 = 1;
const TNM_SEL_ITEM_TYPE_OFF: i64 = 0;
const TNM_SEL_ITEM_TYPE_ON: i64 = 1;
const TNM_SEL_ITEM_TYPE_READ: i64 = 2;

fn parse_selbtn_choices(
    args: &[Value],
) -> (i64, Vec<crate::runtime::globals::BtnSelectChoiceState>) {
    let mut template_no = 0i64;
    let mut start = 0usize;
    if args.first().is_some_and(|v| v.named_id().is_none() && v.as_i64().is_some()) {
        template_no = args.first().and_then(Value::as_i64).unwrap_or(0);
        start = 1;
    }

    let mut out = Vec::new();
    let mut last: Option<usize> = None;
    let mut arg_no = 0i32;
    for v in args.iter().skip(start).filter(|v| v.named_id().is_none()).map(Value::unwrap_named) {
        if let Some(s) = v.as_str() {
            out.push(crate::runtime::globals::BtnSelectChoiceState {
                text: s.to_string(),
                item_type: TNM_SEL_ITEM_TYPE_ON,
                color: -1,
                pos: (0, 0),
                size: (0, 0),
            });
            last = Some(out.len() - 1);
            arg_no = 0;
        } else if let Some(n) = v.as_i64() {
            if let Some(i) = last {
                match arg_no {
                    1 => out[i].item_type = n,
                    2 => out[i].color = n,
                    _ => {}
                }
            }
        }
        arg_no += 1;
    }
    (template_no, out)
}

fn selbtn_text_extent(text: &str, tmpl: &crate::runtime::tables::SelBtnTemplate) -> (i64, i64) {
    let font_px = tmpl.moji_size.max(1);
    let mut width = 0i64;
    for ch in text.chars() {
        let advance = if ch.is_ascii() || matches!(ch as u32, 0xFF61..=0xFF9F) {
            (font_px + tmpl.moji_space.0) / 2
        } else {
            font_px + tmpl.moji_space.0
        };
        width = width.saturating_add(advance.max(1));
    }
    if !text.is_empty() {
        width = width.saturating_sub(tmpl.moji_space.0);
    }
    (width.max(font_px), font_px)
}

fn load_selbtn_image_id(
    ctx: &mut CommandContext,
    file_name: &str,
    patno: u32,
) -> Option<crate::image_manager::ImageId> {
    if file_name.is_empty() {
        return None;
    }
    match ctx.images.load_g00(file_name, patno) {
        Ok(id) => Some(id),
        Err(_) => ctx.images.load_bg_frame(file_name, patno as usize).ok(),
    }
}

fn selbtn_template_item_size(
    ctx: &mut CommandContext,
    choices: &[crate::runtime::globals::BtnSelectChoiceState],
    tmpl: &crate::runtime::tables::SelBtnTemplate,
) -> (i64, i64) {
    if let Some(img_id) = load_selbtn_image_id(ctx, &tmpl.base_file, 0) {
        if let Some(img) = ctx.images.get(img_id) {
            return (img.width as i64, img.height as i64);
        }
    }
    choices
        .first()
        .map(|choice| {
            let (tw, th) = selbtn_text_extent(&choice.text, tmpl);
            (
                tw.saturating_add(tmpl.moji_pos.0.max(0)).max(1),
                th.saturating_add(tmpl.moji_pos.1.max(0)).max(1),
            )
        })
        .unwrap_or((tmpl.moji_size.max(1), tmpl.moji_size.max(1)))
}

fn layout_selbtn_choices(
    choices: &mut [crate::runtime::globals::BtnSelectChoiceState],
    tmpl: &crate::runtime::tables::SelBtnTemplate,
    item_size: (i64, i64),
) {
    let rep_pos = if tmpl.rep_pos == (0, 0) {
        (0, item_size.1.max(tmpl.moji_size.max(1)).max(1))
    } else {
        tmpl.rep_pos
    };

    let mut offset = (0i64, 0i64);
    let mut max_offset = (0i64, 0i64);
    let mut org_offset_x = 0i64;
    let mut y_cnt = 0i64;
    for choice in choices.iter_mut() {
        if choice.item_type != TNM_SEL_ITEM_TYPE_OFF {
            choice.pos = offset;
            choice.size = item_size;
            offset.0 = offset.0.saturating_add(rep_pos.0);
            offset.1 = offset.1.saturating_add(rep_pos.1);
            max_offset.0 = max_offset.0.max(offset.0);
            max_offset.1 = max_offset.1.max(offset.1);
            y_cnt += 1;
            if tmpl.max_y_cnt > 0 && y_cnt >= tmpl.max_y_cnt {
                offset.0 = org_offset_x.saturating_add(tmpl.line_width);
                offset.1 = 0;
                org_offset_x = offset.0;
                y_cnt = 0;
            }
        }
    }

    let total_x = max_offset.0.saturating_sub(rep_pos.0).saturating_add(item_size.0);
    let total_y = max_offset.1.saturating_sub(rep_pos.1).saturating_add(item_size.1);
    let align_x = match tmpl.x_align {
        1 => -total_x / 2,
        2 => -total_x,
        _ => 0,
    };
    let align_y = match tmpl.y_align {
        1 => -total_y / 2,
        2 => -total_y,
        _ => 0,
    };
    for choice in choices.iter_mut() {
        if choice.item_type != TNM_SEL_ITEM_TYPE_OFF {
            choice.pos.0 = choice.pos.0.saturating_add(align_x).saturating_add(tmpl.base_pos.0);
            choice.pos.1 = choice.pos.1.saturating_add(align_y).saturating_add(tmpl.base_pos.1);
        }
    }
}

fn selbtn_table_color(
    tables: &crate::runtime::tables::AssetTables,
    color_no: i64,
    fallback: (u8, u8, u8),
) -> (u8, u8, u8) {
    if color_no < 0 {
        return fallback;
    }
    tables
        .color_table
        .get(color_no as usize)
        .copied()
        .unwrap_or(fallback)
}

fn hide_selbtn_object_backing(ctx: &mut CommandContext, obj: &crate::runtime::globals::ObjectState) {
    match obj.backend {
        crate::runtime::globals::ObjectBackend::Rect { layer_id, sprite_id, .. }
        | crate::runtime::globals::ObjectBackend::String { layer_id, sprite_id, .. }
        | crate::runtime::globals::ObjectBackend::Movie { layer_id, sprite_id, .. } => {
            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                if let Some(sprite) = layer.sprite_mut(sprite_id) {
                    sprite.visible = false;
                    sprite.image_id = None;
                }
            }
        }
        crate::runtime::globals::ObjectBackend::Number { layer_id, ref sprite_ids }
        | crate::runtime::globals::ObjectBackend::Weather { layer_id, ref sprite_ids } => {
            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                for &sprite_id in sprite_ids {
                    if let Some(sprite) = layer.sprite_mut(sprite_id) {
                        sprite.visible = false;
                        sprite.image_id = None;
                    }
                }
            }
        }
        _ => {}
    }
    for child in &obj.runtime.child_objects {
        hide_selbtn_object_backing(ctx, child);
    }
}

fn clear_existing_stage_btnselitems(ctx: &mut CommandContext) {
    let old_items = {
        let Some(st) = ctx.globals.stage_forms.get(&ctx.ids.form_global_stage) else {
            return;
        };
        st.btnselitem_lists
            .get(&TNM_STAGE_FRONT_SELBTN)
            .cloned()
            .unwrap_or_default()
    };
    for item in &old_items {
        for obj in item.generated_objects.iter().chain(item.object_list.iter()) {
            hide_selbtn_object_backing(ctx, obj);
        }
    }
}

fn make_selbtn_image_object(
    ctx: &mut CommandContext,
    file_name: &str,
    patno: u32,
    width: i64,
    height: i64,
    layer_rep: i64,
    item_type: i64,
    _selected: bool,
) -> Option<crate::runtime::globals::ObjectState> {
    let img_id = load_selbtn_image_id(ctx, file_name, patno)?;
    let (img_w, img_h) = ctx
        .images
        .get(img_id)
        .map(|img| (img.width.max(1), img.height.max(1)))
        .unwrap_or((width.max(1) as u32, height.max(1) as u32));
    let layer_id = ctx.layers.create_layer();
    let sprite_id = ctx.layers.layer_mut(layer_id).map(|layer| layer.create_sprite())?;
    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
        if let Some(sprite) = layer.sprite_mut(sprite_id) {
            sprite.fit = crate::layer::SpriteFit::PixelRect;
            sprite.size_mode = crate::layer::SpriteSizeMode::Intrinsic;
            sprite.visible = item_type == TNM_SEL_ITEM_TYPE_ON || item_type == TNM_SEL_ITEM_TYPE_READ;
            sprite.x = 0;
            sprite.y = 0;
            sprite.image_id = Some(img_id);
            sprite.tr = 255;
        }
    }

    let mut obj = crate::runtime::globals::ObjectState::default();
    obj.used = true;
    obj.backend = crate::runtime::globals::ObjectBackend::Rect {
        layer_id,
        sprite_id,
        width: img_w,
        height: img_h,
    };
    obj.object_type = 2;
    obj.file_name = Some(file_name.to_string());
    obj.base.disp = if item_type == TNM_SEL_ITEM_TYPE_ON || item_type == TNM_SEL_ITEM_TYPE_READ { 1 } else { 0 };
    obj.base.x = 0;
    obj.base.y = 0;
    obj.base.patno = patno as i64;
    obj.base.layer = layer_rep;
    if ctx.ids.obj_disp != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, obj.base.disp);
    }
    if ctx.ids.obj_x != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, 0);
    }
    if ctx.ids.obj_y != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, 0);
    }
    if ctx.ids.obj_patno != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, patno as i64);
    }
    if ctx.ids.obj_layer != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_layer, layer_rep);
    }


    Some(obj)
}

fn make_selbtn_text_object(
    ctx: &mut CommandContext,
    choice: &crate::runtime::globals::BtnSelectChoiceState,
    tmpl: &crate::runtime::tables::SelBtnTemplate,
    color_no: i64,
    selected: bool,
) -> Option<crate::runtime::globals::ObjectState> {
    if !(choice.item_type == TNM_SEL_ITEM_TYPE_ON || choice.item_type == TNM_SEL_ITEM_TYPE_READ) || choice.text.is_empty() {
        return None;
    }
    if !ctx.font_cache.is_loaded() {
        let _ = ctx.font_cache.load_for_project(&ctx.project_dir);
    }
    let (tw, th) = selbtn_text_extent(&choice.text, tmpl);
    let width = tw.max(1) as u32;
    let height = th.max(tmpl.moji_size.max(1)) as u32;
    let effective_color_no = if selected && choice.item_type == TNM_SEL_ITEM_TYPE_ON && tmpl.moji_hit_color >= 0 {
        tmpl.moji_hit_color
    } else {
        color_no
    };
    let color = selbtn_table_color(&ctx.tables, effective_color_no, (255, 255, 255));
    let shadow_color = selbtn_table_color(&ctx.tables, ctx.tables.mwnd_render.shadow_color, (0, 0, 0));
    let fuchi_color = selbtn_table_color(&ctx.tables, ctx.tables.mwnd_render.fuchi_color, (0, 0, 0));
    let style = crate::text_render::TextStyle {
        color,
        shadow_color,
        fuchi_color,
        shadow: ctx.tables.font_defaults.shadow == 1 || ctx.tables.font_defaults.shadow == 3,
        fuchi: ctx.tables.font_defaults.shadow == 2 || ctx.tables.font_defaults.shadow == 3,
        bold: ctx.tables.font_defaults.futoku != 0,
    };
    let img_id = ctx.font_cache.render_mwnd_text_styled(
        &mut ctx.images,
        &choice.text,
        tmpl.moji_size.max(1) as f32,
        width,
        height,
        Some(tmpl.moji_space),
        style,
    );
    let layer_id = ctx.layers.create_layer();
    let sprite_id = ctx.layers.layer_mut(layer_id).map(|layer| layer.create_sprite())?;
    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
        if let Some(sprite) = layer.sprite_mut(sprite_id) {
            sprite.fit = crate::layer::SpriteFit::PixelRect;
            sprite.size_mode = if img_id.is_some() {
                crate::layer::SpriteSizeMode::Intrinsic
            } else {
                crate::layer::SpriteSizeMode::Explicit { width, height }
            };
            sprite.visible = true;
            sprite.x = tmpl.moji_pos.0 as i32;
            sprite.y = tmpl.moji_pos.1 as i32;
            sprite.image_id = img_id;
            sprite.tr = 255;
        }
    }
    let mut obj = crate::runtime::globals::ObjectState::default();
    obj.used = true;
    obj.backend = crate::runtime::globals::ObjectBackend::String {
        layer_id,
        sprite_id,
        image_id: img_id,
        width,
        height,
    };
    obj.object_type = 3;
    obj.string_value = Some(choice.text.clone());
    obj.string_param.moji_size = tmpl.moji_size;
    obj.string_param.moji_space_x = tmpl.moji_space.0;
    obj.string_param.moji_space_y = tmpl.moji_space.1;
    obj.string_param.moji_cnt = tmpl.moji_cnt;
    obj.string_param.moji_color = effective_color_no;
    obj.string_param.shadow_color = ctx.tables.mwnd_render.shadow_color;
    obj.string_param.fuchi_color = ctx.tables.mwnd_render.fuchi_color;
    obj.base.disp = 1;
    obj.base.x = tmpl.moji_pos.0;
    obj.base.y = tmpl.moji_pos.1;
    obj.base.layer = ctx.tables.mwnd_render.moji_layer_rep;
    if ctx.ids.obj_disp != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, 1);
    }
    if ctx.ids.obj_x != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, tmpl.moji_pos.0);
    }
    if ctx.ids.obj_y != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, tmpl.moji_pos.1);
    }
    if ctx.ids.obj_layer != 0 {
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_layer, ctx.tables.mwnd_render.moji_layer_rep);
    }
    Some(obj)
}

fn prepare_stage_btnselitems(ctx: &mut CommandContext) {
    clear_existing_stage_btnselitems(ctx);
    let template_no = ctx.globals.selbtn.template_no.max(0) as usize;
    let tmpl = ctx
        .tables
        .sel_btn_templates
        .get(template_no)
        .cloned()
        .unwrap_or_default();
    let choices_for_size = ctx.globals.selbtn.choices.clone();
    let item_size = selbtn_template_item_size(ctx, &choices_for_size, &tmpl);
    layout_selbtn_choices(&mut ctx.globals.selbtn.choices, &tmpl, item_size);
    let choices_snapshot = ctx.globals.selbtn.choices.clone();
    let cursor = ctx.globals.selbtn.cursor;
    let waku_layer_rep = ctx.tables.mwnd_render.waku_layer_rep;
    let filter_layer_rep = ctx.tables.mwnd_render.filter_layer_rep;
    let mut prepared = Vec::with_capacity(choices_snapshot.len());
    for (idx, choice) in choices_snapshot.iter().enumerate() {
        let mut item = crate::runtime::globals::BtnSelItemState::default();
        item.text = choice.text.clone();
        item.item_type = choice.item_type;
        item.color = if choice.color >= 0 { choice.color } else { tmpl.moji_color };
        item.pos = choice.pos;
        item.size = choice.size;
        item.visible = choice.item_type == TNM_SEL_ITEM_TYPE_ON || choice.item_type == TNM_SEL_ITEM_TYPE_READ;
        item.selected = idx == cursor;
        item.button_action_no = tmpl.btn_action_no;
        item.button_state = if choice.item_type == TNM_SEL_ITEM_TYPE_READ {
            4
        } else if item.selected && choice.item_type == TNM_SEL_ITEM_TYPE_ON {
            1
        } else {
            0
        };

        if let Some(obj) = make_selbtn_image_object(
            ctx,
            &tmpl.base_file,
            0,
            item.size.0,
            item.size.1,
            waku_layer_rep,
            choice.item_type,
            item.selected,
        ) {
            item.generated_objects.push(obj);
        }
        if let Some(obj) = make_selbtn_image_object(
            ctx,
            &tmpl.filter_file,
            0,
            item.size.0,
            item.size.1,
            filter_layer_rep,
            choice.item_type,
            item.selected,
        ) {
            item.generated_objects.push(obj);
        }
        if let Some(obj) = make_selbtn_text_object(ctx, choice, &tmpl, item.color, item.selected) {
            item.generated_objects.push(obj);
        }
        prepared.push(item);
    }
    let st = ctx
        .globals
        .stage_forms
        .entry(ctx.ids.form_global_stage)
        .or_default();
    st.btnselitem_lists
        .insert(TNM_STAGE_FRONT_SELBTN, prepared);
}

fn first_selectable_selbtn_choice(choices: &[crate::runtime::globals::BtnSelectChoiceState]) -> usize {
    choices
        .iter()
        .position(|choice| choice.item_type == TNM_SEL_ITEM_TYPE_ON)
        .unwrap_or(0)
}

fn dispatch_selbtn_command(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let op = form_id as i32;
    let ready = op == constants::elm_value::GLOBAL_SELBTN_READY
        || op == constants::elm_value::GLOBAL_SELBTN_CANCEL_READY;
    let start_now = op == constants::elm_value::GLOBAL_SELBTN
        || op == constants::elm_value::GLOBAL_SELBTN_CANCEL
        || op == constants::elm_value::GLOBAL_SELBTN_START;
    if !ready && !start_now {
        return Ok(false);
    }

    if op != constants::elm_value::GLOBAL_SELBTN_START {
        let (template_no, choices) = parse_selbtn_choices(args);
        let capture_flag = named_i64(args, 1).unwrap_or(0) != 0;
        let sel_start_call_scn = args
            .iter()
            .find(|v| v.named_id() == Some(2))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let sel_start_call_z_no = named_i64(args, 3).unwrap_or(0);
        ctx.globals.selbtn.template_no = template_no;
        ctx.globals.selbtn.choices = choices;
        ctx.globals.selbtn.cursor = first_selectable_selbtn_choice(&ctx.globals.selbtn.choices);
        ctx.globals.selbtn.cancel_enable = op == constants::elm_value::GLOBAL_SELBTN_CANCEL
            || op == constants::elm_value::GLOBAL_SELBTN_CANCEL_READY;
        ctx.globals.selbtn.capture_flag = if ready { false } else { capture_flag };
        ctx.globals.selbtn.sel_start_call_scn = if ready {
            String::new()
        } else {
            sel_start_call_scn
        };
        ctx.globals.selbtn.sel_start_call_z_no = if ready { 0 } else { sel_start_call_z_no };
        ctx.globals.selbtn.result = 0;
        prepare_stage_btnselitems(ctx);
    }

    if start_now {
        ctx.globals.selbtn.started = true;
        ctx.globals.selbtn.sync_type = named_i64(args, 4).unwrap_or(0);
        ctx.globals.selbtn.read_flag_scene_no = ctx.current_scene_no.unwrap_or(-1);
        ctx.globals.selbtn.read_flag_flag_no = -1;
        ctx.request_read_flag_no_for_selbtn();
        ctx.wait.wait_key();
    }
    Ok(true)
}

fn global_koe_state_key(_ctx: &CommandContext) -> u32 {
    constants::fm::GLOBAL as u32
}

fn remember_global_koe(ctx: &mut CommandContext, koe_no: i64, chara_no: i64, is_ex: bool) {
    let key = global_koe_state_key(ctx);
    let props = ctx.globals.int_props.entry(key).or_default();
    props.insert(constants::elm_value::GLOBAL_KOE_CHECK_GET_KOE_NO, koe_no);
    props.insert(
        constants::elm_value::GLOBAL_KOE_CHECK_GET_CHARA_NO,
        chara_no,
    );
    props.insert(
        constants::elm_value::GLOBAL_KOE_CHECK_IS_EX_KOE,
        if is_ex { 1 } else { 0 },
    );
}

fn remembered_global_koe(ctx: &CommandContext, op: i32) -> i64 {
    let key = global_koe_state_key(ctx);
    ctx.globals
        .int_props
        .get(&key)
        .and_then(|m| m.get(&op).copied())
        .unwrap_or(0)
}

fn dispatch_global_koe_command(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    let op = form_id as i32;
    let ret_form: Option<i64> = crate::runtime::forms::prop_access::current_vm_meta(ctx).1;
    match op {
        constants::elm_value::GLOBAL_KOE | constants::elm_value::GLOBAL_EXKOE => {
            ctx.request_read_flag_no();
            let is_ex = op == constants::elm_value::GLOBAL_EXKOE;
            let koe_no = if is_ex {
                named_i64(args, 0).or_else(|| positional_i64(args, 0))
            } else {
                positional_i64(args, 0)
            }
            .unwrap_or(0);
            let chara_no = if is_ex {
                named_i64(args, 1).or_else(|| positional_i64(args, 1))
            } else {
                positional_i64(args, 1)
            }
            .unwrap_or(0);
            remember_global_koe(ctx, koe_no, chara_no, is_ex);
            if let Err(err) = {
                let (koe, audio) = (&mut ctx.koe, &mut ctx.audio);
                koe.play_koe_no(audio, koe_no)
            } {
                eprintln!("[SG_AUDIO] koe.play failed koe_no={koe_no}: {err:#}");
            }
            if is_ex && named_i64(args, 2).unwrap_or(0) != 0 {
                let key_skip = named_i64(args, 3).unwrap_or(0) != 0;
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::KoeAny, key_skip);
            }
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_PLAY_WAIT
        | constants::elm_value::GLOBAL_KOE_PLAY_WAIT_KEY
        | constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT
        | constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY => {
            ctx.request_read_flag_no();
            let is_ex = op == constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT
                || op == constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY;
            let koe_no = if is_ex {
                named_i64(args, 0).or_else(|| positional_i64(args, 0))
            } else {
                positional_i64(args, 0)
            }
            .unwrap_or(0);
            let chara_no = if is_ex {
                named_i64(args, 1).or_else(|| positional_i64(args, 1))
            } else {
                positional_i64(args, 1)
            }
            .unwrap_or(0);
            remember_global_koe(ctx, koe_no, chara_no, is_ex);
            if let Err(err) = {
                let (koe, audio) = (&mut ctx.koe, &mut ctx.audio);
                koe.play_koe_no(audio, koe_no)
            } {
                eprintln!("[SG_AUDIO] koe.play_wait failed koe_no={koe_no}: {err:#}");
            }
            let key_skip = op == constants::elm_value::GLOBAL_KOE_PLAY_WAIT_KEY
                || op == constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY;
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::KoeAny, key_skip);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_STOP => {
            let fade = args.get(0).and_then(Value::as_i64);
            let _ = ctx.koe.stop(fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_WAIT | constants::elm_value::GLOBAL_KOE_WAIT_KEY => {
            let key_skip = op == constants::elm_value::GLOBAL_KOE_WAIT_KEY;
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::KoeAny, key_skip);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_CHECK => {
            let playing = ctx.koe.is_playing_any();
            ctx.push(Value::Int(if playing { 1 } else { 0 }));
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_CHECK_GET_KOE_NO
        | constants::elm_value::GLOBAL_KOE_CHECK_GET_CHARA_NO
        | constants::elm_value::GLOBAL_KOE_CHECK_IS_EX_KOE => {
            ctx.push(Value::Int(remembered_global_koe(ctx, op)));
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_SET_VOLUME => {
            let vol = args
                .get(0)
                .and_then(Value::as_i64)
                .unwrap_or(255)
                .clamp(0, 255) as u8;
            let fade = args.get(1).and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.koe.set_volume_raw_fade(&mut ctx.audio, vol, fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_SET_VOLUME_MAX => {
            let fade = args.get(0).and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.koe.set_volume_raw_fade(&mut ctx.audio, 255, fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_SET_VOLUME_MIN => {
            let fade = args.get(0).and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.koe.set_volume_raw_fade(&mut ctx.audio, 0, fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_GET_VOLUME => {
            ctx.push(Value::Int(ctx.koe.volume_raw() as i64));
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn parse_i32_value(v: &Value) -> Option<i32> {
    v.unwrap_named()
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
}

fn parse_bool_value(v: &Value) -> Option<bool> {
    parse_i32_value(v).map(|n| n != 0)
}

fn parse_list_i32_value(v: &Value) -> Vec<i32> {
    match v.unwrap_named() {
        Value::List(xs) => xs
            .iter()
            .filter_map(|x| x.as_i64().and_then(|n| i32::try_from(n).ok()))
            .collect(),
        _ => Vec::new(),
    }
}

fn dispatch_global_fog_command(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    let op = form_id as i32;
    let ret_form = crate::runtime::forms::prop_access::current_vm_meta(ctx)
        .1
        .unwrap_or(0);

    if op == constants::elm_value::GLOBAL___FOG_NAME {
        match ret_form {
            rf if rf == constants::fm::STR as i64 => {
                ctx.push(Value::Str(ctx.globals.fog_global.name.clone()));
            }
            _ => {
                let name = args
                    .first()
                    .and_then(|v| v.unwrap_named().as_str())
                    .unwrap_or("");
                ctx.globals.fog_global = Default::default();
                if !name.is_empty() {
                    match ctx.images.load_g00(name, 0) {
                        Ok(id) => {
                            ctx.globals.fog_global.enabled = true;
                            ctx.globals.fog_global.name = name.to_string();
                            ctx.globals.fog_global.texture_image_id = Some(id);
                        }
                        Err(e) => {
                            log::error!(
                                "GLOBAL.__FOG_NAME failed to load fog texture '{name}': {e}"
                            );
                        }
                    }
                }
            }
        }
        return Ok(true);
    }

    if op == constants::elm_value::GLOBAL___FOG_X {
        if ret_form != 0 {
            ctx.push(Value::Int(ctx.globals.fog_global.x_event.get_value() as i64));
        } else {
            let x = args
                .first()
                .and_then(|v| v.unwrap_named().as_i64())
                .unwrap_or(0) as i32;
            ctx.globals.fog_global.set_x(x);
        }
        return Ok(true);
    }

    if op == constants::elm_value::GLOBAL___FOG_NEAR {
        if ret_form != 0 {
            ctx.push(Value::Int(ctx.globals.fog_global.near as i64));
        } else {
            ctx.globals.fog_global.near = args
                .first()
                .and_then(|v| v.unwrap_named().as_i64())
                .unwrap_or(0) as f32;
        }
        return Ok(true);
    }

    if op == constants::elm_value::GLOBAL___FOG_FAR {
        if ret_form != 0 {
            ctx.push(Value::Int(ctx.globals.fog_global.far as i64));
        } else {
            ctx.globals.fog_global.far = args
                .first()
                .and_then(|v| v.unwrap_named().as_i64())
                .unwrap_or(0) as f32;
        }
        return Ok(true);
    }

    if op != constants::elm_value::GLOBAL___FOG_X_EVE {
        return Ok(false);
    }

    let Some((chain_pos, chain)) =
        crate::runtime::forms::prop_access::parse_element_chain_ctx(ctx, form_id, args)
            .map(|(i, ch)| (i, ch.to_vec()))
    else {
        return Ok(true);
    };
    if chain.len() < 2 {
        return Ok(true);
    }
    let params = &args[..chain_pos];
    match chain[1] {
        int_event_op::SET | int_event_op::SET_REAL => {
            let value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let total_time = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if chain[1] == int_event_op::SET_REAL {
                1
            } else {
                0
            };
            ctx.globals
                .fog_global
                .x_event
                .set_event(value, total_time, delay_time, speed_type, real_flag);
        }
        int_event_op::LOOP | int_event_op::LOOP_REAL => {
            let start_value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if chain[1] == int_event_op::LOOP_REAL {
                1
            } else {
                0
            };
            ctx.globals.fog_global.x_event.loop_event(
                start_value,
                end_value,
                loop_time,
                delay_time,
                speed_type,
                real_flag,
            );
        }
        int_event_op::TURN | int_event_op::TURN_REAL => {
            let start_value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if chain[1] == int_event_op::TURN_REAL {
                1
            } else {
                0
            };
            ctx.globals.fog_global.x_event.turn_event(
                start_value,
                end_value,
                loop_time,
                delay_time,
                speed_type,
                real_flag,
            );
        }
        int_event_op::END => ctx.globals.fog_global.x_event.end_event(),
        int_event_op::WAIT => ctx.wait.wait_fog_x_event(false, false),
        int_event_op::WAIT_KEY => ctx.wait.wait_fog_x_event(true, true),
        int_event_op::CHECK => {
            ctx.push(Value::Int(
                if ctx.globals.fog_global.x_event.check_event() {
                    1
                } else {
                    0
                },
            ));
        }
        _ => {}
    }
    Ok(true)
}

fn resolve_wipe_mask_path(project_dir: &Path, raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let norm = raw.replace('\\', "/");
    let p = Path::new(&norm);
    if p.is_absolute() && p.is_file() {
        return Some(p.to_path_buf());
    }
    let mut candidates = Vec::new();
    candidates.push(project_dir.join(&norm));
    candidates.push(project_dir.join("dat").join(&norm));
    if p.extension().is_none() {
        for ext in ["png", "bmp", "jpg"] {
            candidates.push(project_dir.join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("dat").join(format!("{}.{}", norm, ext)));
        }
    }
    candidates.into_iter().find(|c| c.is_file())
}

fn dispatch_global_wipe_command(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    let op = form_id as i32;
    let is_mask = matches!(
        op,
        constants::elm_value::GLOBAL_MASK_WIPE | constants::elm_value::GLOBAL_MASK_WIPE_ALL
    );
    let is_all = matches!(
        op,
        constants::elm_value::GLOBAL_WIPE_ALL | constants::elm_value::GLOBAL_MASK_WIPE_ALL
    );

    if op == constants::elm_value::GLOBAL_WIPE_END {
        ctx.globals.finish_wipe();
        return Ok(true);
    }
    if op == constants::elm_value::GLOBAL_WAIT_WIPE {
        let key_wait_mode = args
            .iter()
            .find_map(|v| match v {
                Value::NamedArg { id, value } if *id == 0 => parse_i32_value(value),
                _ => None,
            })
            .unwrap_or(-1);
        let key_skip = match key_wait_mode {
            0 => false,
            1 => true,
            _ => {
                ctx.globals
                    .syscom
                    .config_int
                    .get(&197)
                    .copied()
                    .unwrap_or(0)
                    != 0
            }
        };
        ctx.wait.wait_wipe(key_skip);
        return Ok(true);
    }
    if op == constants::elm_value::GLOBAL_CHECK_WIPE {
        ctx.push(Value::Int(if ctx.globals.wipe_done() { 0 } else { 1 }));
        return Ok(true);
    }

    if !matches!(
        op,
        constants::elm_value::GLOBAL_WIPE
            | constants::elm_value::GLOBAL_WIPE_ALL
            | constants::elm_value::GLOBAL_MASK_WIPE
            | constants::elm_value::GLOBAL_MASK_WIPE_ALL
    ) {
        return Ok(false);
    }

    let mut positional: Vec<&Value> = Vec::new();
    let mut named: Vec<(i32, &Value)> = Vec::new();
    for a in args {
        match a {
            Value::NamedArg { id, value } => named.push((*id, value.as_ref())),
            _ => positional.push(a),
        }
    }

    let mut mask_file: Option<String> = None;
    let mut wipe_type: i32 = 0;
    let mut wipe_time: i32 = 500;
    let mut speed_mode: i32 = 0;
    let mut start_time: i32 = 0;
    let mut option: Vec<i32> = Vec::new();
    let mut begin_order: i32 = 0;
    let mut end_order: i32 = if is_all { i32::MAX } else { 0 };
    let mut begin_layer: i32 = i32::MIN;
    let mut end_layer: i32 = i32::MAX;
    let mut wait_flag = true;
    let mut key_wait_mode: i32 = -1;
    let mut with_low_order: i32 = 0;

    if is_mask {
        mask_file = positional
            .get(0)
            .and_then(|v| v.unwrap_named().as_str())
            .map(str::to_string);
        if let Some(v) = positional.get(1).and_then(|v| parse_i32_value(v)) {
            wipe_type = v;
        }
        if let Some(v) = positional.get(2).and_then(|v| parse_i32_value(v)) {
            wipe_time = v;
        }
        if let Some(v) = positional.get(3).and_then(|v| parse_i32_value(v)) {
            speed_mode = v;
        }
        if let Some(v) = positional.get(4) {
            option = parse_list_i32_value(v);
        }
    } else {
        if let Some(v) = positional.get(0).and_then(|v| parse_i32_value(v)) {
            wipe_type = v;
        }
        if let Some(v) = positional.get(1).and_then(|v| parse_i32_value(v)) {
            wipe_time = v;
        }
        if let Some(v) = positional.get(2).and_then(|v| parse_i32_value(v)) {
            speed_mode = v;
        }
        if let Some(v) = positional.get(3) {
            option = parse_list_i32_value(v);
        }
    }

    for (id, v) in named {
        match id {
            0 => {
                if let Some(x) = parse_i32_value(v) {
                    wipe_type = x;
                }
            }
            1 => {
                if let Some(x) = parse_i32_value(v) {
                    wipe_time = x;
                }
            }
            2 => {
                if let Some(x) = parse_i32_value(v) {
                    speed_mode = x;
                }
            }
            3 => option = parse_list_i32_value(v),
            4 => {
                if let Some(x) = parse_i32_value(v) {
                    begin_order = x;
                }
            }
            5 => {
                if let Some(x) = parse_i32_value(v) {
                    end_order = x;
                }
            }
            6 => {
                if let Some(x) = parse_i32_value(v) {
                    begin_layer = x;
                }
            }
            7 => {
                if let Some(x) = parse_i32_value(v) {
                    end_layer = x;
                }
            }
            8 => {
                if let Some(x) = parse_bool_value(v) {
                    wait_flag = x;
                }
            }
            9 => {
                if let Some(x) = parse_i32_value(v) {
                    key_wait_mode = x;
                }
            }
            10 => {
                if let Some(x) = parse_i32_value(v) {
                    with_low_order = x;
                }
            }
            11 => {
                if let Some(x) = parse_i32_value(v) {
                    start_time = x;
                }
            }
            _ => {}
        }
    }
    if is_all {
        end_order = i32::MAX;
    }

    let mask_image_id = mask_file.as_ref().and_then(|f| {
        resolve_wipe_mask_path(&ctx.project_dir, f).and_then(|p| ctx.images.load_file(&p, 0).ok())
    });

    stage::apply_stage_wipe(ctx, begin_order, end_order, begin_layer, end_layer);
    ctx.globals.start_wipe(WipeState::new(
        mask_file,
        mask_image_id,
        wipe_type,
        wipe_time,
        start_time,
        speed_mode,
        option,
        begin_order,
        end_order,
        begin_layer,
        end_layer,
        wait_flag,
        key_wait_mode,
        with_low_order,
    ));

    if wait_flag {
        let key_skip = match key_wait_mode {
            0 => false,
            1 => true,
            _ => {
                ctx.globals
                    .syscom
                    .config_int
                    .get(&197)
                    .copied()
                    .unwrap_or(0)
                    != 0
            }
        };
        ctx.wait.wait_wipe(key_skip);
    }
    Ok(true)
}

fn dispatch_capture_command(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    match form_id as i32 {
        constants::elm_value::GLOBAL_CAPTURE => {
            let img = ctx.capture_frame_rgba();
            ctx.globals.capture_image = Some(img.clone());
            crate::runtime::forms::syscom::prepare_runtime_save_thumb_capture(ctx);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FREE => {
            ctx.globals.capture_image = None;
            ctx.globals.save_thumb_capture_image = None;
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FROM_FILE => {
            let Some(file) = args.get(0).and_then(|v| v.as_str()) else {
                panic!("GLOBAL.CAPTURE_FROM_FILE requires file name");
            };
            let Some(path) =
                stage::resolve_capture_file_path(&ctx.project_dir, &ctx.globals.append_dir, file)
            else {
                panic!("GLOBAL.CAPTURE_FROM_FILE cannot resolve file: {file}");
            };
            let img_id = ctx.images.load_file(&path, 0).unwrap_or_else(|e| {
                panic!(
                    "GLOBAL.CAPTURE_FROM_FILE failed to load {}: {e}",
                    path.display()
                )
            });
            let img = ctx
                .images
                .get(img_id)
                .map(|img| img.as_ref().clone())
                .unwrap_or_else(|| {
                    panic!(
                        "GLOBAL.CAPTURE_FROM_FILE image disappeared: {}",
                        path.display()
                    )
                });
            crate::runtime::forms::syscom::prepare_runtime_save_thumb_capture_from_image(ctx, &img);
            ctx.globals.capture_image = Some(img);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FOR_OBJECT => {
            let has_range = named_i64(args, 0).is_some() || named_i64(args, 1).is_some();
            let img = if has_range {
                let end_order = named_i64(args, 0).unwrap_or(i32::MAX as i64 / 1024);
                let end_layer = named_i64(args, 1).unwrap_or(1023);
                ctx.capture_frame_rgba_until(end_order, end_layer)
            } else {
                ctx.capture_frame_rgba()
            };
            ctx.globals.capture_for_object_image = Some(img);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FOR_OBJECT_FREE => {
            ctx.globals.capture_for_object_image = None;
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FOR_LOCAL_SAVE => {
            let end_order = named_i64(args, 0).unwrap_or(i32::MAX as i64 / 1024);
            let end_layer = named_i64(args, 1).unwrap_or(1023);
            let width = named_i64(args, 3).unwrap_or(ctx.screen_w as i64).max(1) as u32;
            let height = named_i64(args, 4).unwrap_or(ctx.screen_h as i64).max(1) as u32;
            let img = ctx.capture_frame_rgba_until(end_order, end_layer);
            let resized = crate::runtime::forms::syscom::resize_capture_rgba_nearest(
                &img,
                width,
                height,
            );
            ctx.globals.capture_for_object_image = Some(resized);
            ctx.push(Value::Int(1));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FOR_TWEET => {
            let img = ctx.capture_frame_rgba();
            ctx.globals.capture_image = Some(img);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FREE_FOR_TWEET => {
            ctx.globals.capture_image = None;
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn push_global_message_ok(ctx: &mut CommandContext) {
    let ret_form = crate::runtime::forms::prop_access::current_vm_meta(ctx)
        .1
        .unwrap_or(0);
    if ret_form == 0 {
        return;
    }
    if ret_form == constants::fm::STR as i64 {
        ctx.push(Value::Str(String::new()));
    } else {
        ctx.push(Value::Int(0));
    }
}

fn global_message_arg_str(args: &[Value]) -> Option<&str> {
    args.iter().rev().find_map(|v| v.unwrap_named().as_str())
}

fn dispatch_global_message_command(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    match form_id as i32 {
        constants::elm_value::GLOBAL_MESSAGE_BOX => {
            let text = global_message_arg_str(args).unwrap_or("").to_string();
            ctx.request_system_messagebox_no_return(
                17,
                false,
                text,
                vec![crate::runtime::globals::SystemMessageBoxButton {
                    label: "OK".to_string(),
                    value: 0,
                }],
            );
            Ok(true)
        }
        constants::elm_value::GLOBAL_GET_LAST_SEL_MSG => {
            ctx.push(Value::Str(ctx.globals.syscom.system_extra_str_value.clone()));
            Ok(true)
        }
        constants::elm_value::GLOBAL_OPEN
        | constants::elm_value::GLOBAL_OPEN_WAIT
        | constants::elm_value::GLOBAL_OPEN_NOWAIT => {
            ctx.ui.show_message_bg(true);
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_CLOSE
        | constants::elm_value::GLOBAL_CLOSE_WAIT
        | constants::elm_value::GLOBAL_CLOSE_NOWAIT => {
            ctx.ui.show_message_bg(false);
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_END_CLOSE => {
            // C++ GLOBAL.END_CLOSE dispatches MWND.END_CLOSE for the current
            // message window. If the stage/current-MWND route above did not
            // handle it, this fallback must not perform CLOSE semantics.
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_MSG_BLOCK | constants::elm_value::GLOBAL_MSG_PP_BLOCK => {
            // Message block commands update/forward message state only. They must not
            // create a script-proc boundary; WAIT_MSG / PP / R / PAGE are the commands
            // that actually stop the running script.
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_CLEAR => {
            ctx.ui.clear_message();
            ctx.ui.clear_name();
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_CLEAR_MSGBK => {
            ctx.ui.clear_message();
            let form_id = ctx.ids.form_global_msgbk;
            if form_id != 0 {
                ctx.globals.msgbk_forms.entry(form_id).or_default().clear();
            }
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_PRINT => {
            ctx.request_read_flag_no();
            if let Some(s) = global_message_arg_str(args) {
                if !s.is_empty() {
                    syscom::append_current_save_message(ctx, s);
                    ctx.ui.show_message_bg(true);
                    ctx.ui.append_message(s);
                }
            }
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_NL | constants::elm_value::GLOBAL_NLI => {
            ctx.ui.append_linebreak();
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_WAIT_MSG | constants::elm_value::GLOBAL_PP => {
            ctx.ui.begin_wait_message();
            ctx.wait.wait_key();
            ctx.request_message_wait_proc_boundary();
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_R | constants::elm_value::GLOBAL_PAGE => {
            if (form_id as i32) == constants::elm_value::GLOBAL_PAGE {
                ctx.ui.begin_wait_page_message();
            } else {
                ctx.ui.begin_wait_message();
            }
            ctx.ui.request_clear_message_on_wait_end();
            ctx.wait.wait_key();
            ctx.request_message_wait_proc_boundary();
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_SET_NAMAE => {
            let name = global_message_arg_str(args).unwrap_or("");
            if !stage::cd_name_current_mwnd(ctx, name) {
                ctx.ui.set_name(name.to_string());
            }
            push_global_message_ok(ctx);
            Ok(true)
        }
        constants::elm_value::GLOBAL_CLEAR_FACE
        | constants::elm_value::GLOBAL_SET_FACE
        | constants::elm_value::GLOBAL_SIZE
        | constants::elm_value::GLOBAL_COLOR
        | constants::elm_value::GLOBAL_RUBY
        | constants::elm_value::GLOBAL_MSGBTN
        | constants::elm_value::GLOBAL_MULTI_MSG
        | constants::elm_value::GLOBAL_NEXT_MSG
        | constants::elm_value::GLOBAL_START_SLIDE_MSG
        | constants::elm_value::GLOBAL_END_SLIDE_MSG
        | constants::elm_value::GLOBAL_INDENT
        | constants::elm_value::GLOBAL_CLEAR_INDENT
        | constants::elm_value::GLOBAL_REP_POS
        | constants::elm_value::GLOBAL_SET_WAKU => {
            push_global_message_ok(ctx);
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub fn dispatch_global_form(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    let form_id = canonical_global_form_id(ctx, form_id);

    if dispatch_global_wipe_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if dispatch_capture_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if dispatch_global_fog_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if dispatch_selbtn_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if stage::dispatch_current_mwnd_global_op(ctx, form_id as i32, args) {
        return Ok(true);
    }
    if dispatch_global_koe_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if dispatch_global_message_command(ctx, form_id, args)? {
        return Ok(true);
    }

    // Same-version testcase still uses compact startup aliases that bypass the
    // canonical global-form ids. Keep them routed to their original handlers.
    if form_id == 24 {
        return keylist::dispatch(ctx, args);
    }
    if form_id == 40 {
        return counter::dispatch(ctx, form_id, args);
    }
    if form_id == 63 {
        if syscom::dispatch(ctx, form_id, args)? {
            return Ok(true);
        }
    }
    if form_id == 64 {
        if script::dispatch(ctx, form_id, args)? {
            return Ok(true);
        }
    }
    if form_id == 46 {
        return mouse::dispatch(ctx, args);
    }
    if form_id == 86 {
        if input::dispatch(ctx, form_id, args)? {
            return Ok(true);
        }
    }
    if form_id == 92 {
        if system::dispatch(ctx, form_id, args)? {
            return Ok(true);
        }
    }
    if form_id == constants::elm_value::GLOBAL_DISP as u32 {
        ctx.wait.wait_next_frame(ctx.globals.render_frame);
        ctx.request_disp_proc_boundary();
        return Ok(true);
    }
    if form_id == constants::elm_value::GLOBAL_FRAME as u32 {
        ctx.wait.wait_next_frame(ctx.globals.render_frame);
        ctx.request_proc_boundary(crate::runtime::ProcKind::Frame);
        return Ok(true);
    }
    if form_id == constants::elm_value::GLOBAL_SET_MWND as u32
        || form_id == constants::elm_value::GLOBAL_SET_SEL_MWND as u32
    {
        let next = args.iter().find_map(mwnd_ref_from_value);
        if form_id == constants::elm_value::GLOBAL_SET_SEL_MWND as u32 {
            if let Some((stage, no)) = next {
                ctx.globals.current_sel_mwnd_stage_idx = stage;
                ctx.globals.current_sel_mwnd_no = Some(no);
            }
        } else if let Some((stage, no)) = next {
            ctx.globals.current_mwnd_stage_idx = stage;
            ctx.globals.current_mwnd_no = Some(no);
            ctx.globals.last_mwnd_stage_idx = stage;
            ctx.globals.last_mwnd_no = Some(no);
        }
        return Ok(true);
    }
    if form_id == constants::elm_value::GLOBAL_GET_MWND as u32
        || form_id == constants::elm_value::GLOBAL_GET_SEL_MWND as u32
    {
        let no = if form_id == constants::elm_value::GLOBAL_GET_SEL_MWND as u32 {
            ctx.globals.current_sel_mwnd_no
        } else {
            ctx.globals.current_mwnd_no
        };
        // C++ returns -1 only when no current MWND element resolves.
        ctx.push(Value::Int(no.map(|n| n as i64).unwrap_or(-1)));
        return Ok(true);
    }
    if form_id == constants::elm_value::GLOBAL_SET_TITLE as u32 {
        // Original engine forwards this to the OS window caption.  The winit
        // shell owns the actual window, so keep VM semantics as a successful
        // side-effect command here.
        return Ok(true);
    }

    if form_id == constants::global_form::STAGE_ALT {
        return stage::dispatch(ctx, args);
    }
    if form_id == constants::global_form::BGM {
        return forms::bgm::dispatch(ctx, args);
    }
    if form_id == constants::global_form::BGMTABLE {
        return forms::bgm_table::dispatch(ctx, args);
    }
    if form_id == constants::global_form::MOV {
        return forms::mov::dispatch(ctx, args);
    }
    if form_id == constants::global_form::PCM {
        return forms::pcm::dispatch(ctx, args);
    }
    if form_id == constants::global_form::PCMCH {
        return forms::pcmch::dispatch(ctx, form_id, args);
    }
    if form_id == constants::global_form::SE {
        return forms::se::dispatch(ctx, args);
    }
    if form_id == constants::global_form::PCMEVENT {
        return forms::pcmevent::dispatch(ctx, args);
    }
    if form_id == constants::global_form::EXCALL {
        return forms::excall::dispatch(ctx, args);
    }
    if form_id == constants::global_form::KOE_ST {
        return forms::koe_st::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_input {
        return input::dispatch(ctx, form_id, args);
    }
    if form_id == ctx.ids.form_global_mouse {
        return mouse::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_keylist {
        return keylist::dispatch(ctx, args);
    }
    if form_id == constants::global_form::KEY {
        return key::dispatch(ctx, args);
    }
    if form_id == constants::global_form::SCREEN {
        return forms::screen::dispatch(ctx, args);
    }
    if form_id == constants::global_form::MSGBK {
        return forms::msgbk::dispatch(ctx, args);
    }
    if ctx.ids.form_global_math != 0 && form_id == ctx.ids.form_global_math {
        return math::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_cgtable != 0 && form_id == ctx.ids.form_global_cgtable {
        return cgtable::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_database != 0 && form_id == ctx.ids.form_global_database {
        return database::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_g00buf != 0 && form_id == ctx.ids.form_global_g00buf {
        return g00buf::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_mask != 0 && form_id == ctx.ids.form_global_mask {
        return mask::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_editbox != 0 && form_id == ctx.ids.form_global_editbox {
        return editbox::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_file != 0 && form_id == ctx.ids.form_global_file {
        return file::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_steam != 0 && form_id == ctx.ids.form_global_steam {
        return steam::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_syscom != 0 && form_id == ctx.ids.form_global_syscom {
        return syscom::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_script != 0 && form_id == ctx.ids.form_global_script {
        return script::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_system != 0 && form_id == ctx.ids.form_global_system {
        return system::dispatch(ctx, form_id, args);
    }
    if form_id == constants::global_form::FRAME_ACTION {
        return frame_action::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_frame_action_ch != 0 && form_id == ctx.ids.form_global_frame_action_ch {
        return frame_action_ch::dispatch(ctx, form_id, args);
    }

    match form_id {
        constants::global_form::BGM => forms::bgm::dispatch(ctx, args),
        constants::global_form::BGMTABLE => forms::bgm_table::dispatch(ctx, args),
        constants::global_form::MOV => forms::mov::dispatch(ctx, args),
        constants::global_form::PCM => forms::pcm::dispatch(ctx, args),
        constants::global_form::PCMCH => forms::pcmch::dispatch(ctx, form_id, args),
        constants::global_form::SE => forms::se::dispatch(ctx, args),
        constants::global_form::PCMEVENT => forms::pcmevent::dispatch(ctx, args),
        constants::global_form::EXCALL => forms::excall::dispatch(ctx, args),
        constants::global_form::KOE_ST => forms::koe_st::dispatch(ctx, args),
        constants::global_form::SCREEN => forms::screen::dispatch(ctx, args),
        constants::global_form::MSGBK => forms::msgbk::dispatch(ctx, args),
        constants::global_form::KEY => key::dispatch(ctx, args),
        _ => {
            // TIMEWAIT/TIMEWAIT_KEY are statement-like forms that block execution.
            if form_id == constants::global_form::TIMEWAIT {
                return timewait::dispatch(ctx, false, args);
            }
            if form_id == constants::global_form::TIMEWAIT_KEY {
                return timewait::dispatch(ctx, true, args);
            }

            if form_id as i32 == constants::fm::INTEVENT
                || form_id as i32 == constants::fm::INTEVENTLIST
            {
                return int_event::dispatch(ctx, form_id, args);
            }

            if form_id as i32 == constants::fm::OBJECTEVENT {
                return object_event::dispatch(ctx, args);
            }
            if form_id as i32 == crate::runtime::forms::codes::FM_OBJECTEVENTLIST {
                return object_event::dispatch_list(ctx, args);
            }

            if constants::global_form::INT_LIST_FORMS.contains(&form_id) {
                return int_list::dispatch(ctx, form_id, args);
            }
            if constants::global_form::STR_LIST_FORMS.contains(&form_id) {
                return str_list::dispatch(ctx, form_id, args);
            }

            if form_id == constants::global_form::COUNTER {
                return counter::dispatch(ctx, form_id, args);
            }

            if form_id == constants::global_form::FRAME_ACTION {
                return int_list::dispatch(ctx, form_id, args);
            }

            Ok(false)
        }
    }
}
