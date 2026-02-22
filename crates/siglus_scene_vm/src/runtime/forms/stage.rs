//! Global Stage form handler.
//!
//! This module implements a pragmatic subset of Siglus "Stage" form behavior.
//! The original engine routes a large portion of UI and sprite control through
//! Stage form (Stage/Object/MWND/Group). We implement:
//!   - Stage list count
//!   - Stage.OBJECT[*] subset (wired to `GfxRuntime`)
//!   - Stage.MWND[*] bring-up (text / wait)
//!   - Stage.OBJBTNGROUP[*] bring-up (selection / wait)
//!
//! IMPORTANT: Many element/op numeric codes are not mapped for all games.
//! For non-object stage children we auto-learn op meanings by (arg signature + return form)
//! and cache op-id -> semantic kind. This keeps the VM progressing without requiring
//! external "element code" dumps.

use anyhow::Result;

use std::path::{Path, PathBuf};

use crate::runtime::globals::{
    GroupListOpKind, GroupOpKind, MwndListOpKind, MwndOpKind,
    MsgBackAtom, MsgBackState,
    ObjectBackend, ObjectCompatState, ObjectListOpKind, ObjectOpKind, ObjectWeatherParam,
    StageChildKind, StageFormState,
};
use crate::runtime::Value;
use crate::runtime::int_event::IntEvent;
use crate::layer::{SpriteFit, SpriteId, SpriteSizeMode};

use super::super::CommandContext;

#[derive(Debug, Clone)]
enum StageTarget {
    StageCount,
    /// [FORM_STAGE, ELM_ARRAY, stage_idx, op]
    StageOp { stage: i64, op: i64 },
    /// [FORM_STAGE, ELM_ARRAY, stage_idx, child_code, op]
    ChildListOp { stage: i64, child: i32, op: i64 },
    /// [FORM_STAGE, ELM_ARRAY, stage_idx, child_code, ELM_ARRAY, idx, op, tail...]
    ChildItemOp { stage: i64, child: i32, idx: i64, op: i64, tail: Vec<i32> },
}

fn parse_target(ctx: &CommandContext, chain: &[i32]) -> Option<StageTarget> {
    // Stage count: [FORM_STAGE, 1]
    if chain.len() == 2 && chain[0] == ctx.ids.form_global_stage as i32 && chain[1] as i64 == 1 {
        return Some(StageTarget::StageCount);
    }

    // Common shape:
    //   [FORM_STAGE, ELM_ARRAY, stage_idx, ...]
    if chain.len() < 4 {
        return None;
    }
    if chain[0] != ctx.ids.form_global_stage as i32 {
        return None;
    }
    if chain[1] != ctx.ids.elm_array {
        return None;
    }
    let stage = chain[2] as i64;

    if chain.len() == 4 {
        return Some(StageTarget::StageOp {
            stage,
            op: chain[3] as i64,
        });
    }

    let child = chain[3] as i32;

    if chain.len() == 5 {
        return Some(StageTarget::ChildListOp {
            stage,
            child,
            op: chain[4] as i64,
        });
    }

    if chain.len() >= 7 && chain[4] == ctx.ids.elm_array {
        return Some(StageTarget::ChildItemOp {
            stage,
            child,
            idx: chain[5] as i64,
            op: chain[6] as i64,
            tail: chain.get(7..).unwrap_or(&[]).to_vec(),
        });
    }

    None
}

fn find_chain(args: &[Value]) -> Option<(usize, Vec<i32>)> {
    for (i, v) in args.iter().enumerate().rev() {
        if let Value::Element(e) = v {
            return Some((i, e.clone()));
        }
    }
    None
}

fn as_i64(v: &Value) -> Option<i64> {
    v.as_i64()
}

fn as_str(v: &Value) -> Option<&str> {
    v.as_str()
}

fn default_for_ret_form(ret_form: i64) -> Value {
    // Bring-up heuristic used across the port: ret_form==2 is string.
    if ret_form == 2 {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn push_ok(ctx: &mut CommandContext, ret_form: Option<i64>) {
    match ret_form {
        Some(0) | None => ctx.stack.push(Value::Int(0)),
        Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
    }
}

fn ensure_default_msg_bg(ctx: &mut CommandContext) {
    if ctx.ui.msg_bg_image.is_some() {
        return;
    }
    // Simple placeholder (1x1) scaled by the UI layout logic.
    let img = ctx.images.solid_rgba((0, 0, 0, 160));
    ctx.ui.set_message_bg(img);
}

fn looks_like_path(s: &str) -> bool {
    if s.contains('/') || s.contains('\\') {
        return true;
    }
    if s.contains(':') {
        // Windows drive letter / URL scheme / etc.
        return true;
    }
    let lower = s.to_ascii_lowercase();
    for ext in [
        ".png", ".bmp", ".jpg", ".jpeg", ".tga", ".dds", ".webp", ".tif", ".tiff",
        ".g00", ".g02", ".g03",
    ] {
        if lower.ends_with(ext) {
            return true;
        }
    }
    false
}

fn name_candidate(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if looks_like_path(s) {
        return false;
    }
    if s.contains('\n') || s.contains('\r') {
        return false;
    }
    // Heuristic: short and no obvious sentence punctuation.
    let len = s.chars().count();
    if len > 16 {
        return false;
    }
    !s.contains('。') && !s.contains('！') && !s.contains('？') && !s.contains('.')
}

fn try_set_ui_bg_from_name(ctx: &mut CommandContext, name: &str) {
    if name.is_empty() {
        return;
    }

    // Best-effort: try direct file, then g00, then bg.
    if ctx.images.load_file(Path::new(name), 0).map(|id| {
        ctx.ui.set_message_bg(id);
    }).is_ok() {
        return;
    }
    if ctx.images.load_g00(name, 0).map(|id| {
        ctx.ui.set_message_bg(id);
    }).is_ok() {
        return;
    }
    let _ = ctx.images.load_bg(name).map(|id| {
        ctx.ui.set_message_bg(id);
    });
}

fn stage_state_mut(ctx: &mut CommandContext, form_id: u32) -> &mut StageFormState {
    ctx.globals.stage_forms.entry(form_id).or_default()
}

fn with_stage_state<R>(
    ctx: &mut CommandContext,
    form_id: u32,
    f: impl FnOnce(&mut CommandContext, &mut StageFormState) -> R,
) -> R {
    let mut st = ctx.globals.stage_forms.remove(&form_id).unwrap_or_default();
    let r = f(ctx, &mut st);
    ctx.globals.stage_forms.insert(form_id, st);
    r
}


fn msgbk_state_mut(ctx: &mut CommandContext) -> Option<&mut MsgBackState> {
    let form_id = ctx.ids.form_global_msgbk;
    if form_id == 0 {
        return None;
    }
    Some(ctx.globals.msgbk_forms.entry(form_id).or_default())
}

fn msgbk_add_text(ctx: &mut CommandContext, s: &str) {
    let Some(st) = msgbk_state_mut(ctx) else { return; };
    if s.is_empty() {
        return;
    }
    st.cur.atoms.push(MsgBackAtom::Text(s.to_string()));
}

fn msgbk_add_name(ctx: &mut CommandContext, s: &str) {
    let Some(st) = msgbk_state_mut(ctx) else { return; };
    // The original engine records name separately; for bring-up we store it as a distinct atom.
    st.cur.atoms.push(MsgBackAtom::Name(s.to_string()));
}

fn msgbk_next(ctx: &mut CommandContext) {
    let Some(st) = msgbk_state_mut(ctx) else { return; };
    st.next();
}

fn ensure_group(ctx: &mut CommandContext, st: &mut StageFormState, stage_idx: i64, group_idx: usize) {
    let _ = ctx;
    st.ensure_group_list(stage_idx, group_idx + 1);
    let list = st.group_lists.get_mut(&stage_idx).unwrap();
    let g = &mut list[group_idx];
    // On first touch, initialize to the same "empty" defaults used by the original engine.
    if g.hit_button_no == 0 && g.pushed_button_no == 0 && g.decided_button_no == 0 && g.result == 0 && g.result_button_no == 0 {
        g.init_sel();
        g.order = 0;
        g.layer = 0;
        g.cancel_priority = 0;
    }
}

fn ensure_mwnd(ctx: &mut CommandContext, st: &mut StageFormState, stage_idx: i64, mwnd_idx: usize) {
    st.ensure_mwnd_list(stage_idx, mwnd_idx + 1);
    let _ = ctx;
    let _ = st;
}

// -----------------------------------------------------------------------------
// OBJECT / OBJECTLIST
// -----------------------------------------------------------------------------

fn learn_object_list_op(
    st: &mut StageFormState,
    op: i32,
    argc: usize,
    ret_form: Option<i64>,
) -> ObjectListOpKind {
    if let Some(&k) = st.object_list_op_map.get(&op) {
        return k;
    }

    let k = if argc == 0 && ret_form.unwrap_or(1) != 0 {
        ObjectListOpKind::GetSize
    } else if argc == 1 {
        ObjectListOpKind::Resize
    } else {
        ObjectListOpKind::Unknown
    };
    st.object_list_op_map.insert(op, k);
    k
}

fn dispatch_object_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let k = learn_object_list_op(st, op, script_args.len(), ret_form);
    match k {
        ObjectListOpKind::GetSize => {
            ctx.stack.push(Value::Int(st.object_list_len(stage_idx) as i64));
            true
        }
        ObjectListOpKind::Resize => {
            let Some(n0) = script_args.get(0).and_then(as_i64) else {
                push_ok(ctx, ret_form);
                return true;
            };
            let n = if n0 < 0 { 0 } else { n0 as usize };

            let old_len = st.object_list_len(stage_idx);
            if n < old_len {
                if let Some(list) = st.object_lists.get_mut(&stage_idx) {
                    for i in n..old_len {
                        let obj = &mut list[i];
                        object_clear_backend(ctx, obj, stage_idx, i);
                        *obj = ObjectCompatState::default();
                    }
                }
            }

            st.set_object_list_len_strict(stage_idx, n);
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectListOpKind::Unknown => false,
    }
}

fn ensure_object_for_access(st: &mut StageFormState, stage_idx: i64, obj_idx: usize) -> bool {
    let strict = st.object_list_strict.get(&stage_idx).copied().unwrap_or(false);
    let entry = st.object_lists.entry(stage_idx).or_default();
    if entry.len() <= obj_idx {
        if strict {
            return false;
        }
        entry.extend((0..(obj_idx + 1 - entry.len())).map(|_| ObjectCompatState::default()));
    }
    true
}

fn ensure_rect_layer(ctx: &mut CommandContext, st: &mut StageFormState, stage_idx: i64) -> usize {
    if let Some(&id) = st.rect_layers.get(&stage_idx) {
        return id;
    }
    let id = ctx.layers.create_layer();
    st.rect_layers.insert(stage_idx, id);
    id
}

fn object_clear_backend(ctx: &mut CommandContext, obj: &mut ObjectCompatState, stage_idx: i64, obj_idx: usize) {
    if matches!(obj.backend, ObjectBackend::Gfx) {
		let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
		let _ = gfx.object_clear(images, layers, stage_idx, obj_idx as i64);
    }
    if let ObjectBackend::Rect { layer_id, sprite_id, .. } = obj.backend {
        if let Some(layer) = ctx.layers.layer_mut(layer_id) {
            if let Some(spr) = layer.sprite_mut(sprite_id) {
                spr.visible = false;
                spr.image_id = None;
            }
        }
    }
    if let ObjectBackend::Number { layer_id, ref sprite_ids } = obj.backend {
        if let Some(layer) = ctx.layers.layer_mut(layer_id) {
            for &sid in sprite_ids {
                if let Some(spr) = layer.sprite_mut(sid) {
                    spr.visible = false;
                    spr.image_id = None;
                }
            }
        }
    }
    obj.backend = ObjectBackend::None;
}


const TNM_SCALE_UNIT: i64 = 1000;
const TNM_BTN_STATE_NORMAL: i64 = 0;
const TNM_BTN_STATE_HIT: i64 = 1;
const TNM_BTN_STATE_PUSH: i64 = 2;
const TNM_BTN_STATE_SELECT: i64 = 3;
const TNM_BTN_STATE_DISABLE: i64 = 4;

fn split_pos_named<'a>(args: &'a [Value]) -> (Vec<&'a Value>, Vec<(i32, &'a Value)>) {
    let mut pos = Vec::new();
    let mut named = Vec::new();
    for a in args {
        if let Value::NamedArg { id, value } = a {
            named.push((*id, value.as_ref()));
        } else {
            pos.push(a);
        }
    }
    (pos, named)
}

fn resolve_omv_path(project_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let p = Path::new(file_name);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }

    let mov_dir = project_dir.join("mov");

    if file_name.contains('.') {
        let cand = mov_dir.join(file_name);
        if cand.exists() {
            return Some(cand);
        }
    }

    let cand = mov_dir.join(format!("{}.omv", file_name));
    if cand.exists() {
        return Some(cand);
    }

    None
}

fn omv_total_time_ms(path: &Path) -> Option<u64> {
    let omv = siglus_assets::omv::OmvFile::open(path).ok()?;
    let fps = omv.header.fps as u64;
    let frames = omv.header.frame_count as u64;
    if fps == 0 {
        return None;
    }
    Some(frames.saturating_mul(1000) / fps)
}

fn digits_most_significant(mut n: u64) -> Vec<i64> {
    if n == 0 {
        return vec![0];
    }
    let mut d = Vec::new();
    while n > 0 {
        d.push((n % 10) as i64);
        n /= 10;
    }
    d.reverse();
    d
}

fn update_number_backend(ctx: &mut CommandContext, obj: &mut ObjectCompatState) {
    let (layer_id, sprite_ids) = match &obj.backend {
        ObjectBackend::Number { layer_id, sprite_ids } => (*layer_id, sprite_ids.as_slice()),
        _ => return,
    };

    let Some(file) = obj.file_name.as_deref() else {
        if let Some(layer) = ctx.layers.layer_mut(layer_id) {
            for &sid in sprite_ids {
                if let Some(spr) = layer.sprite_mut(sid) {
                    spr.visible = false;
                    spr.image_id = None;
                }
            }
        }
        return;
    };

    let disp = if ctx.ids.obj_disp != 0 {
        obj.extra_int_props.get(&ctx.ids.obj_disp).copied().unwrap_or(0) != 0
    } else {
        true
    };
    let base_x = if ctx.ids.obj_x != 0 {
        obj.extra_int_props.get(&ctx.ids.obj_x).copied().unwrap_or(0) as i32
    } else {
        0
    };
    let base_y = if ctx.ids.obj_y != 0 {
        obj.extra_int_props.get(&ctx.ids.obj_y).copied().unwrap_or(0) as i32
    } else {
        0
    };
    let base_pat = if ctx.ids.obj_patno != 0 {
        obj.extra_int_props.get(&ctx.ids.obj_patno).copied().unwrap_or(0)
    } else {
        0
    };

    let keta_max = obj.number_param.keta_max.max(0).min(16) as usize;
    let disp_zero = obj.number_param.disp_zero != 0;
    let disp_sign_cfg = obj.number_param.disp_sign != 0;
    let tumeru_sign = obj.number_param.tumeru_sign != 0 && !disp_zero;
    let space_mod = obj.number_param.space_mod;
    let space = obj.number_param.space;

    let n = obj.number_value;
    let sign = if n == 0 { 0 } else if n > 0 { 1 } else { -1 };
    let disp_sign = disp_sign_cfg || sign == -1;

    let digits = digits_most_significant(n.unsigned_abs());
    let keta = digits.len();

    let mut pat_no = [0i64; 16];
    let mut spr_disp = [false; 16];

    if disp_zero {
        for i in 0..keta_max {
            spr_disp[i] = true;
            pat_no[i] = 0;
        }
    }

    let mut num_pos = keta_max.saturating_sub(keta);
    let mut sign_pos: Option<usize> = None;

    if disp_sign {
        let sp = if tumeru_sign {
            num_pos.saturating_sub(1)
        } else {
            0
        };
        sign_pos = Some(sp);
        num_pos = num_pos.max(sp + 1);
    }

    for (i, d) in digits.iter().enumerate() {
        let idx = num_pos + i;
        if idx < 16 {
            pat_no[idx] = *d;
            spr_disp[idx] = true;
        }
    }

    if let Some(sp) = sign_pos {
        let p = match sign {
            0 => 12,
            1 => 11,
            -1 => 10,
            _ => 0,
        };
        if sp < 16 {
            pat_no[sp] = p;
            spr_disp[sp] = true;
        }
    }

    for i in 0..16 {
        pat_no[i] = pat_no[i].saturating_add(base_pat);
    }

    // Width used for spacing when space_mod==0.
    let default_w = ctx
        .images
        .load_g00(file, base_pat.max(0) as u32)
        .ok()
        .and_then(|id| ctx.images.get(id).map(|img| img.width as i32));

    let mut offset: i32 = 0;

    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
        for (i, &sid) in sprite_ids.iter().enumerate().take(16) {
            let frame = pat_no[i].max(0) as u32;
            let img_id = ctx.images.load_g00(file, frame).ok();

            let w = img_id
                .and_then(|id| ctx.images.get(id).map(|img| img.width as i32))
                .or(default_w)
                .unwrap_or(0);

            if let Some(spr) = layer.sprite_mut(sid) {
                spr.fit = SpriteFit::PixelRect;
                spr.size_mode = SpriteSizeMode::Intrinsic;
                spr.order = i as i32;
                spr.x = base_x - offset;
                spr.y = base_y;
                spr.visible = disp && spr_disp[i] && img_id.is_some();
                spr.image_id = img_id;
            }

            offset = offset.saturating_add(space as i32);
            if space_mod == 0 {
                offset = offset.saturating_add(w);
            }
        }
    }
}

fn learn_object_op(
    st: &mut StageFormState,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
    rhs: Option<&Value>,
    al_id: Option<i64>,
) -> ObjectOpKind {
    if let Some(&k) = st.object_op_map.get(&op) {
        return k;
    }

    if matches!(rhs, Some(Value::Str(_))) || ret_form == Some(2) {
        st.object_str_ops.insert(op);
    }

    // Do not map generic properties; only map command-like ops.
    if rhs.is_some() || ret_form.is_none() {
        return ObjectOpKind::Unknown;
    }

    let has_str = script_args.iter().any(|v| matches!(v, Value::Str(_)));
    let all_int = script_args.iter().all(|v| v.as_i64().is_some());

    let k = if all_int && ret_form == Some(0) && script_args.len() >= 2 {
        // multi-arg setters (SET_POS/SET_CENTER/SET_SCALE/SET_ROTATE/SET_CLIP/SET_SRC_CLIP)
        // We do not assume numeric IDs; we learn the first occurrences by signature.
        let used = st.object_op_map.values().copied().collect::<Vec<_>>();
        if (2..=3).contains(&script_args.len()) {
            if !used.contains(&ObjectOpKind::SetPos) {
                ObjectOpKind::SetPos
            } else if !used.contains(&ObjectOpKind::SetCenter) {
                ObjectOpKind::SetCenter
            } else if !used.contains(&ObjectOpKind::SetScale) {
                ObjectOpKind::SetScale
            } else if !used.contains(&ObjectOpKind::SetRotate) {
                ObjectOpKind::SetRotate
            } else {
                ObjectOpKind::Unknown
            }
        } else if script_args.len() == 4 {
            if !used.contains(&ObjectOpKind::SetClip) {
                ObjectOpKind::SetClip
            } else if !used.contains(&ObjectOpKind::SetSrcClip) {
                ObjectOpKind::SetSrcClip
            } else {
                ObjectOpKind::Unknown
            }
        } else {
            ObjectOpKind::Unknown
        }
    } else if has_str {
        let s0 = script_args.iter().find_map(as_str).unwrap_or("");
        if looks_like_path(s0) {
            ObjectOpKind::CreatePct
        } else {
            // For one-arg string commands we handle behavior in dispatch; keep this unmapped.
            ObjectOpKind::CreateString
        }
    } else if script_args.len() >= 8 {
        ObjectOpKind::CreateRect
    } else if script_args.is_empty() && ret_form == Some(0) {
        if !st.object_op_map.values().any(|&v| v == ObjectOpKind::Init) {
            ObjectOpKind::Init
        } else if !st.object_op_map.values().any(|&v| v == ObjectOpKind::Free) {
            ObjectOpKind::Free
        } else if !st.object_op_map.values().any(|&v| v == ObjectOpKind::InitParam) {
            ObjectOpKind::InitParam
        } else {
            ObjectOpKind::Unknown
        }
    } else {
        ObjectOpKind::Unknown
    };

    let _ = al_id;

    if k != ObjectOpKind::Unknown {
        st.object_op_map.insert(op, k);
    }
    k
}


struct ObjectWriteBack {
    st: *mut StageFormState,
    stage_idx: i64,
    obj_u: usize,
    obj: ObjectCompatState,
}

impl Drop for ObjectWriteBack {
    fn drop(&mut self) {
        unsafe {
            let st = &mut *self.st;
            if let Some(list) = st.object_lists.get_mut(&self.stage_idx) {
                if self.obj_u < list.len() {
                    let obj = std::mem::replace(&mut self.obj, ObjectCompatState::default());
                    list[self.obj_u] = obj;
                }
            }
        }
    }
}

fn dispatch_object_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    obj_idx: i64,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    ret_form: Option<i64>,
    rhs: Option<&Value>,
    al_id: Option<i64>,
) -> bool {
    if obj_idx < 0 {
        push_ok(ctx, ret_form);
        return true;
    }
    let obj_u = obj_idx as usize;

    if !ensure_object_for_access(st, stage_idx, obj_u) {
        // Strict out-of-range: return default based on ret_form if present.
        match ret_form {
            Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
            None => ctx.stack.push(Value::Int(0)),
        }
        return true;
    }

    // Best-effort: support CREATE_COPY_FROM (one Element argument) by copying compat state.
    // We avoid sharing backend resources (sprites) across objects.
    let mut copy_from_snapshot: Option<(i64, usize, ObjectCompatState)> = None;
    if rhs.is_none() && ret_form == Some(0) && script_args.len() == 1 {
        if let Value::Element(e) = &script_args[0] {
            if let Some(StageTarget::ChildItemOp { stage, child, idx, .. }) = parse_target(ctx, e) {
                if child == ctx.ids.stage_elm_object && idx >= 0 {
                    let src_u = idx as usize;
                    if let Some(src_list) = st.object_lists.get(&stage) {
                        if let Some(src_obj) = src_list.get(src_u) {
                            copy_from_snapshot = Some((stage, src_u, src_obj.clone()));
                        }
                    }
                }
            }
        }
    }



    let obj0 = {
        let list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut list[obj_u])
    };
    let mut obj_write_back = ObjectWriteBack {
        st: st as *mut StageFormState,
        stage_idx,
        obj_u,
        obj: obj0,
    };
    let obj = &mut obj_write_back.obj;

    if let Some((src_stage, src_idx, mut src)) = copy_from_snapshot.take() {
        // Reset destination.
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        let src_file = src.file_name.clone();
        src.backend = ObjectBackend::None;
        if let Some(file) = src.file_name.clone() {
            let disp = ctx.gfx.object_peek_disp(src_stage, src_idx as i64).unwrap_or(0) != 0;
            let (x, y) = ctx.gfx.object_peek_pos(src_stage, src_idx as i64).unwrap_or((0, 0));
            let pat = ctx.gfx.object_peek_patno(src_stage, src_idx as i64).unwrap_or(0);
			{
				let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
				let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, &file, disp as i64, x, y, pat);
			}
            src.backend = ObjectBackend::Gfx;
        }
        // Preserve file name even if gfx binding fails.
        if src.file_name.is_none() {
            src.file_name = src_file;
        }
        *obj = src;
        obj.used = true;
        push_ok(ctx, ret_form);
        return true;
    }

    if !tail.is_empty() || obj.extra_events.contains_key(&op) || obj.rep_int_event_lists.contains_key(&op) {
        let mut arr_idx: Option<i64> = None;
        let mut t = tail;
        if t.len() >= 2 && (t[0] == ctx.ids.elm_array || t[0] == -1) {
            arr_idx = Some(t[1] as i64);
            t = &t[2..];
        }

        // List-level ops.
        if arr_idx.is_none() && t.len() == 1 {
            match t[0] {
                3 => {
                    if let Some(n0) = script_args.get(0).and_then(as_i64) {
                        // OBJECT.F is a fixed-size (32) int list.
                        if ctx.ids.obj_f != 0 && op == ctx.ids.obj_f {
                            // Ignore resize.
                        } else {
                            let n = if n0 < 0 { 0 } else { n0 as usize };
                            obj.rep_int_lists.entry(op).or_default().resize(n, 0);
                        }
                    }
                    ctx.stack.push(Value::Int(0));
                    return true;
                }
                4 => {
                    let n = obj.rep_int_lists.get(&op).map(|v| v.len()).unwrap_or(0);
                    ctx.stack.push(Value::Int(n as i64));
                    return true;
                }
                1 => {
                    // INT_EVENT_LIST resize uses tail=1 with one arg.
                    if script_args.len() == 1 {
                        if let Some(n0) = script_args.get(0).and_then(as_i64) {
                            let n = if n0 < 0 { 0 } else { n0 as usize };
                            obj.rep_int_event_lists
                                .entry(op)
                                .or_default()
                                .resize_with(n, || IntEvent::new(0));
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                }
                _ => {}
            }
        }

        // ALL_EVE: 0=END, 1=WAIT, 2=CHECK
        if arr_idx.is_none()
            && t.len() == 1
            && (0..=2).contains(&t[0])
            && script_args.is_empty()
            && rhs.is_none()
        {
            match t[0] {
                0 => {
                    obj.end_all_events();
                    push_ok(ctx, ret_form);
                    return true;
                }
                1 => {
                    if obj.any_event_active() {
                        ctx.wait
                            .wait_object_all_events(ctx.ids.form_global_stage, stage_idx, obj_u, false);
                    }
                    push_ok(ctx, ret_form);
                    return true;
                }
                2 => {
                    ctx.stack.push(Value::Int(if obj.any_event_active() { 1 } else { 0 }));
                    return true;
                }
                _ => {}
            }
        }

        // Array access.
        if let Some(rep_idx) = arr_idx {
            if rep_idx < 0 {
                ctx.stack.push(Value::Int(0));
                return true;
            }
            let ri = rep_idx as usize;

            let looks_like_event = !t.is_empty() || obj.rep_int_event_lists.contains_key(&op);
            if looks_like_event {
                let ent = obj.rep_int_event_lists.entry(op).or_default();
                if ent.len() <= ri {
                    ent.resize_with(ri + 1, || IntEvent::new(0));
                }
                let ev = &mut ent[ri];

                if t.is_empty() {
                    if let Some(Value::Int(v)) = rhs {
                        ev.set_value(*v as i32);
                        ctx.stack.push(Value::Int(0));
                    } else {
                        ctx.stack.push(Value::Int(ev.get_value() as i64));
                    }
                    return true;
                }

                match t[0] {
                    0 => {
                        if script_args.len() >= 4 {
                            let v = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let tt = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let d = (script_args.get(2).and_then(as_i64).unwrap_or(0)) as i32;
                            let sp = (script_args.get(3).and_then(as_i64).unwrap_or(0)) as i32;
                            ev.set_event(v, tt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    1 => {
                        if script_args.len() >= 5 {
                            let sv = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let evv = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let lt = (script_args.get(2).and_then(as_i64).unwrap_or(0)) as i32;
                            let d = (script_args.get(3).and_then(as_i64).unwrap_or(0)) as i32;
                            let sp = (script_args.get(4).and_then(as_i64).unwrap_or(0)) as i32;
                            ev.loop_event(sv, evv, lt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    2 => {
                        if ret_form.unwrap_or(0) != 0 {
                            ctx.stack.push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        } else {
                            ev.end_event();
                            ctx.stack.push(Value::Int(0));
                        }
                        return true;
                    }
                    3 => {
                        if ev.check_event() {
                            ctx.wait.wait_object_event_list(
                                ctx.ids.form_global_stage,
                                stage_idx,
                                obj_u,
                                op,
                                ri,
                                false,
                            );
                        }
                        push_ok(ctx, ret_form);
                        return true;
                    }
                    4 => {
                        ctx.stack.push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        return true;
                    }
                    _ => {}
                }
            }

            // Default: INT_LIST element access.
            let ent = if ctx.ids.obj_f != 0 && op == ctx.ids.obj_f {
                obj.rep_int_lists.entry(op).or_insert_with(|| vec![0; 32])
            } else {
                obj.rep_int_lists.entry(op).or_default()
            };
            if ent.len() <= ri {
                // For fixed-size lists (OBJECT.F), clamp to the maximum length.
                if ctx.ids.obj_f != 0 && op == ctx.ids.obj_f {
                    // Out of range: behave as 0.
                    ctx.stack.push(Value::Int(0));
                    return true;
                }
                ent.resize(ri + 1, 0);
            }
            if let Some(Value::Int(v)) = rhs {
                ent[ri] = *v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(ent[ri]));
            }
            return true;
        }

        // Non-list INT_EVENT.
        if arr_idx.is_none() {
            if t.is_empty() {
                if obj.extra_events.contains_key(&op) {
                    let ev = obj.extra_events.entry(op).or_insert_with(|| IntEvent::new(0));
                    if let Some(Value::Int(v)) = rhs {
                        ev.set_value(*v as i32);
                        ctx.stack.push(Value::Int(0));
                    } else {
                        ctx.stack.push(Value::Int(ev.get_value() as i64));
                    }
                    return true;
                }
            } else if (0..=4).contains(&t[0]) {
                let ev = obj.extra_events.entry(op).or_insert_with(|| IntEvent::new(0));
                match t[0] {
                    0 => {
                        if script_args.len() >= 4 {
                            let v = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let tt = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let d = (script_args.get(2).and_then(as_i64).unwrap_or(0)) as i32;
                            let sp = (script_args.get(3).and_then(as_i64).unwrap_or(0)) as i32;
                            ev.set_event(v, tt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    1 => {
                        if script_args.len() >= 5 {
                            let sv = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let evv = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let lt = (script_args.get(2).and_then(as_i64).unwrap_or(0)) as i32;
                            let d = (script_args.get(3).and_then(as_i64).unwrap_or(0)) as i32;
                            let sp = (script_args.get(4).and_then(as_i64).unwrap_or(0)) as i32;
                            ev.loop_event(sv, evv, lt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    2 => {
                        if ret_form.unwrap_or(0) != 0 {
                            ctx.stack.push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        } else {
                            ev.end_event();
                            ctx.stack.push(Value::Int(0));
                        }
                        return true;
                    }
                    3 => {
                        if ev.check_event() {
                            ctx.wait
                                .wait_object_event(ctx.ids.form_global_stage, stage_idx, obj_u, op, false);
                        }
                        push_ok(ctx, ret_form);
                        return true;
                    }
                    4 => {
                        ctx.stack.push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        return true;
                    }
                    _ => {}
                }
            }
        }
    }

    // OBJECT.ALL_EVE.{END,WAIT,CHECK}
    // The element chain is OBJECT[...].ALL_EVE.ALLEVENT_*(no args).
    // We support it when the numeric IDs are provided via IdMap.
    if op == ctx.ids.obj_all_eve {
        let sub = tail.get(0).copied().unwrap_or(0);
        if sub == ctx.ids.elm_allevent_end {
            obj.end_all_events();
            push_ok(ctx, ret_form);
            return true;
        }
        if sub == ctx.ids.elm_allevent_wait {
            if obj.any_event_active() {
                ctx.wait
                    .wait_object_all_events(ctx.ids.form_global_stage, stage_idx, obj_u, false);
            }
            push_ok(ctx, ret_form);
            return true;
        }
        if sub == ctx.ids.elm_allevent_check {
            ctx.stack.push(Value::Int(if obj.any_event_active() { 1 } else { 0 }));
            return true;
        }
    }

    // Keep the existing id-mapped subset for correctness when ids are available.

// Id-mapped subset: when numeric ids are available in IdMap.
    // This must reflect actual runtime state for both Gfx and Rect backends.
    if op == ctx.ids.obj_init {
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.backend = ObjectBackend::None;
        obj.file_name = None;
        obj.string_value = None;
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_free {
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        *obj = ObjectCompatState::default();
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_init_param {
        // The bring-up port does not implement def-param tables yet;
        // keep as a no-op but do not break scripts.
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_get_file_name {
        let s = obj.file_name.clone().unwrap_or_default();
        ctx.stack.push(Value::Str(s));
        return true;
    }

    if op == ctx.ids.obj_create {
        let Some(file) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };

        // optional args depend on al_id; here we derive from argc.
        let disp = if script_args.len() >= 2 {
            script_args.get(1).and_then(as_i64).unwrap_or(0) != 0
        } else {
            false
        };
        let (x, y) = if script_args.len() >= 4 {
            (
                script_args.get(2).and_then(as_i64).unwrap_or(0),
                script_args.get(3).and_then(as_i64).unwrap_or(0),
            )
        } else {
            (0, 0)
        };
        let patno = if script_args.len() >= 5 {
            script_args.get(4).and_then(as_i64).unwrap_or(0)
        } else {
            0
        };

        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();

		{
			let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
			let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, file, disp as i64, x, y, patno);
		}
        obj.used = true;
        obj.backend = ObjectBackend::Gfx;
        obj.file_name = Some(file.to_string());
        obj.string_value = None;
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_disp {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            let b = v != 0;
            match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.visible = b;
                        }
                    }
                }
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_disp(images, layers, stage_idx, obj_u as i64, if b { 1 } else { 0 });
                }
                ObjectBackend::Number { .. } => {
                    obj.extra_int_props.insert(op, if b { 1 } else { 0 });
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.extra_int_props.insert(op, if b { 1 } else { 0 });
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| if spr.visible { 1 } else { 0 })
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx.gfx.object_peek_disp(stage_idx, obj_u as i64).unwrap_or(0),
                ObjectBackend::Number { .. } => *obj.extra_int_props.get(&op).unwrap_or(&0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_x {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.x = v as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_x(images, layers, stage_idx, obj_u as i64, v);
                }
                ObjectBackend::Number { .. } => {
                    obj.extra_int_props.insert(op, v);
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.extra_int_props.insert(op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.x as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_pos(stage_idx, obj_u as i64)
                    .map(|(x, _)| x)
                    .unwrap_or(0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_y {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.y = v as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_y(images, layers, stage_idx, obj_u as i64, v);
                }
                ObjectBackend::Number { .. } => {
                    obj.extra_int_props.insert(op, v);
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.extra_int_props.insert(op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.y as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_pos(stage_idx, obj_u as i64)
                    .map(|(_, y)| y)
                    .unwrap_or(0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_z {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            // The bring-up renderer does not use Z for sorting (project constraint),
            // Keep Z in sync for callers that treat it as a property.
            if obj.backend == ObjectBackend::Gfx {
                let _ = ctx.gfx.object_set_z(stage_idx, obj_u as i64, v);
            }
            obj.extra_int_props.insert(op, v);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(*obj.extra_int_props.get(&op).unwrap_or(&0)));
        }
        return true;
    }

    if op == ctx.ids.obj_world {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            obj.extra_int_props.insert(op, v);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(*obj.extra_int_props.get(&op).unwrap_or(&0)));
        }
        return true;
    }

    if op == ctx.ids.obj_patno {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_pat_no(images, layers, stage_idx, obj_u as i64, v);
                }
                ObjectBackend::Number { .. } => {
                    obj.extra_int_props.insert(op, v);
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.extra_int_props.insert(op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Gfx => ctx.gfx.object_peek_patno(stage_idx, obj_u as i64).unwrap_or(0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_layer {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_layer(images, layers, stage_idx, obj_u as i64, v);
                }
                _ => {
                    obj.extra_int_props.insert(op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Gfx => ctx.gfx.object_peek_layer(stage_idx, obj_u as i64).unwrap_or(0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_alpha {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            let a = v.clamp(0, 255) as u8;
            match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.alpha = a;
                        }
                    }
                }
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_alpha(images, layers, stage_idx, obj_u as i64, i64::from(a));
                }
                _ => {
                    obj.extra_int_props.insert(op, a as i64);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.alpha as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx.gfx.object_peek_alpha(stage_idx, obj_u as i64).unwrap_or(0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_order {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.order = v as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_order(images, layers, stage_idx, obj_u as i64, v);
                }
                _ => {
                    obj.extra_int_props.insert(op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.order as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx.gfx.object_peek_order(stage_idx, obj_u as i64).unwrap_or(0),
                _ => *obj.extra_int_props.get(&op).unwrap_or(&0),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    // Properties that exist in IdMap but have no visual backend: store as generic ints.
    if op == ctx.ids.obj_wipe_copy || op == ctx.ids.obj_wipe_erase || op == ctx.ids.obj_click_disable {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            obj.extra_int_props.insert(op, v);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(*obj.extra_int_props.get(&op).unwrap_or(&0)));
        }
        return true;
    }

    // Generic property-style access via call_command():
    // The engine uses `al_id` 0/1 for many get/set properties.
    if rhs.is_none() {
        if let (Some(aid), Some(rf)) = (al_id, ret_form) {
            // Getter: no args.
            if aid == 0 && script_args.is_empty() {
                if rf == 2 || st.object_str_ops.contains(&op) {
                    if let Some(s) = obj.extra_str_props.get(&op) {
                        ctx.stack.push(Value::Str(s.clone()));
                    } else if let Some(f) = &obj.file_name {
                        ctx.stack.push(Value::Str(f.clone()));
                    } else if let Some(t) = &obj.string_value {
                        ctx.stack.push(Value::Str(t.clone()));
                    } else {
                        ctx.stack.push(Value::Str(String::new()));
                    }
                } else if rf != 0 {
                    ctx.stack.push(Value::Int(*obj.extra_int_props.get(&op).unwrap_or(&0)));
                } else {
                    ctx.stack.push(Value::Int(0));
                }
                return true;
            }
            // Setter: one arg.
            if aid == 1 && script_args.len() == 1 {
                if let Some(v) = script_args.get(0).and_then(as_i64) {
                    obj.extra_int_props.insert(op, v);
                    ctx.stack.push(Value::Int(0));
                    return true;
                }
                if let Some(s) = script_args.get(0).and_then(as_str) {
                    obj.extra_str_props.insert(op, s.to_string());
                    st.object_str_ops.insert(op);
                    ctx.stack.push(Value::Int(0));
                    return true;
                }
            }
        }
    }

    // Single-arg command compatibility (SET_STRING/SET_NUMBER/CHANGE_FILE/...):
    // If we cannot map the op, we still preserve state to keep scripts progressing.
    if rhs.is_none() && ret_form == Some(0) && script_args.len() == 1 {
        if let Some(s) = script_args.get(0).and_then(as_str) {
            if looks_like_path(s) {
                // Heuristic: first use of a path on an unused object is a create; otherwise treat as change_file.
                if !obj.used {
                    object_clear_backend(ctx, obj, stage_idx, obj_u);
                    obj.extra_int_props.clear();
                    obj.extra_str_props.clear();
                    obj.extra_events.clear();
                    obj.rep_int_lists.clear();
                    obj.rep_int_event_lists.clear();
                    obj.cmd_int_args.clear();
					{
						let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
						let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, s, 0, 0, 0, 0);
					}
                    obj.used = true;
                    obj.backend = ObjectBackend::Gfx;
                    obj.file_name = Some(s.to_string());
                    obj.string_value = None;
                } else if matches!(obj.backend, ObjectBackend::Gfx) {
                    // Best-effort: preserve current base params.
                    let disp = ctx.gfx.object_peek_disp(stage_idx, obj_u as i64).unwrap_or(0) != 0;
                    let (x, y) = ctx.gfx.object_peek_pos(stage_idx, obj_u as i64).unwrap_or((0, 0));
                    let pat = ctx.gfx.object_peek_patno(stage_idx, obj_u as i64).unwrap_or(0);
					{
						let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
						let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, s, disp as i64, x, y, pat);
					}
                    obj.file_name = Some(s.to_string());
                } else {
                    obj.file_name = Some(s.to_string());
                }
            } else {
                // Treat as SET_STRING for string objects.
                obj.used = true;
                obj.string_value = Some(s.to_string());
                obj.extra_str_props.insert(op, s.to_string());
                st.object_str_ops.insert(op);
            }
            ctx.stack.push(Value::Int(0));
            return true;
        }
        if let Some(v) = script_args.get(0).and_then(as_i64) {
            obj.extra_int_props.insert(op, v);
            obj.cmd_int_args.insert(op, vec![v]);
            ctx.stack.push(Value::Int(0));
            return true;
        }
    }

    if rhs.is_none() && ret_form == Some(2) && script_args.is_empty() {
        // Best-effort string getter.
        if let Some(s) = obj.extra_str_props.get(&op) {
            ctx.stack.push(Value::Str(s.clone()));
        } else if let Some(t) = &obj.string_value {
            ctx.stack.push(Value::Str(t.clone()));
        } else if let Some(f) = &obj.file_name {
            ctx.stack.push(Value::Str(f.clone()));
        } else {
            ctx.stack.push(Value::Str(String::new()));
        }
        return true;
    }



    // ---------------------------------------------------------------------
    // Button (minimal bring-up)
    // ---------------------------------------------------------------------

    // Detect SET_BUTTON_GROUP(element) by a unique signature: al_id==1 and one Element argument.
    if rhs.is_none() && ret_form == Some(0) && al_id == Some(1) && script_args.len() == 1 {
        if let Value::Element(e) = &script_args[0] {
            // Expected element chain: STAGE[stage].(group-child)[idx]
            if let Some(StageTarget::ChildItemOp { idx: gidx, .. }) = parse_target(ctx, e) {
                let gidx_u = if gidx < 0 { 0 } else { gidx as usize };
                obj.button.group_idx_override = Some(gidx_u);
                // Cache mapping so subsequent int-based calls can reuse.
                st.object_op_map.insert(op, ObjectOpKind::SetButtonGroup);
                ctx.stack.push(Value::Int(0));
                return true;
            }
        }
    }

    // Detect SET_BUTTON by presence of al_id==2 (4 ints: button_no, group_no, action_no, se_no).
    // Once observed, cache op-id -> SetButton and accept al_id 0/1 variants for the same op.
    if rhs.is_none() && ret_form == Some(0) {
        if let Some(ObjectOpKind::SetButton) = st.object_op_map.get(&op).copied() {
            // fallthrough below
        } else if al_id == Some(2)
            && (1..=4).contains(&script_args.len())
            && script_args.iter().all(|v| v.as_i64().is_some())
        {
            st.object_op_map.insert(op, ObjectOpKind::SetButton);
        }

        if let Some(ObjectOpKind::SetButton) = st.object_op_map.get(&op).copied() {
            // Mirror SET_BUTTON fallthrough by al_id.
            let mut button_no = 0i64;
            let mut group_no = 0i64;
            let mut action_no = 0i64;
            let mut se_no = 0i64;

            let ints = [
                script_args.get(0).and_then(as_i64).unwrap_or(0),
                script_args.get(1).and_then(as_i64).unwrap_or(0),
                script_args.get(2).and_then(as_i64).unwrap_or(0),
                script_args.get(3).and_then(as_i64).unwrap_or(0),
            ];

            match al_id.unwrap_or(0) {
                2 => {
                    se_no = ints[3];
                    action_no = ints[2];
                    group_no = ints[1];
                    button_no = ints[0];
                }
                1 => {
                    group_no = ints[1];
                    button_no = ints[0];
                }
                _ => {
                    button_no = ints[0];
                }
            }

            obj.button.enabled = true;
            obj.button.button_no = button_no;
            obj.button.group_no = group_no;
            obj.button.action_no = action_no;
            obj.button.se_no = se_no;
            // Best-effort: clear runtime hover state when params change.
            obj.button.hit = false;
            obj.button.pushed = false;

            // Preserve raw args for later reverse engineering.
            let mut raw = Vec::new();
            for i in 0..script_args.len().min(4) {
                raw.push(ints[i]);
            }
            obj.cmd_int_args.insert(op, raw);

            ctx.stack.push(Value::Int(0));
            return true;
        }
    }

    // CLEAR_BUTTON: if a 0-arg void op is invoked after a button is enabled, treat it as CLEAR_BUTTON.
    if rhs.is_none() && ret_form == Some(0) && script_args.is_empty() && obj.button.enabled {
        // Avoid clobbering INIT/FREE/INIT_PARAM (those are already mapped explicitly).
        let mapped = st.object_op_map.get(&op).copied().unwrap_or(ObjectOpKind::Unknown);
        if matches!(mapped, ObjectOpKind::Unknown) {
            st.object_op_map.insert(op, ObjectOpKind::ClearButton);
        }
        if matches!(st.object_op_map.get(&op).copied(), Some(ObjectOpKind::ClearButton)) {
            obj.button.clear();
            ctx.stack.push(Value::Int(0));
            return true;
        }
    }
    // ---------------------------------------------------------------------
    // Direct translations (ID-mapped element codes)
    //
    // IMPORTANT: Many numeric IDs are game-specific. For any id-map entry that
    // defaults to 0 (unknown), we *must not* match it to avoid hijacking op=0.
    // ---------------------------------------------------------------------

    if ctx.ids.obj_exist_type != 0 && op == ctx.ids.obj_exist_type {
        ctx.stack.push(Value::Int(if obj.object_type == 0 { 0 } else { 1 }));
        return true;
    }

    if ctx.ids.obj_change_file != 0 && op == ctx.ids.obj_change_file {
        let Some(name) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };
        // CHANGE_FILE does not reinit; it swaps the underlying file.
        obj.file_name = Some(name.to_string());
        if matches!(obj.backend, ObjectBackend::Gfx) {
            let disp = ctx.gfx.object_peek_disp(stage_idx, obj_u as i64).unwrap_or(0) != 0;
            let (x, y) = ctx.gfx.object_peek_pos(stage_idx, obj_u as i64).unwrap_or((0, 0));
            let pat = ctx.gfx.object_peek_patno(stage_idx, obj_u as i64).unwrap_or(0);
			{
				let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
				let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, name, disp as i64, x, y, pat);
			}
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_string != 0 && op == ctx.ids.obj_set_string {
        let Some(v) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };
        obj.string_value = Some(v.to_string());
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_string != 0 && op == ctx.ids.obj_get_string {
        ctx.stack.push(Value::Str(obj.string_value.clone().unwrap_or_default()));
        return true;
    }
    if ctx.ids.obj_set_string_param != 0 && op == ctx.ids.obj_set_string_param {
        // SET_STRING_PARAM
        // base: (moji_size, space_x, space_y, moji_cnt)
        obj.string_param.moji_size = script_args.get(0).and_then(as_i64).unwrap_or(obj.string_param.moji_size);
        obj.string_param.moji_space_x = script_args.get(1).and_then(as_i64).unwrap_or(obj.string_param.moji_space_x);
        obj.string_param.moji_space_y = script_args.get(2).and_then(as_i64).unwrap_or(obj.string_param.moji_space_y);
        obj.string_param.moji_cnt = script_args.get(3).and_then(as_i64).unwrap_or(obj.string_param.moji_cnt);
        // optional: (moji_color, shadow_color, shadow_mode, fuchi_color)
        if script_args.len() >= 7 {
            obj.string_param.moji_color = script_args.get(4).and_then(as_i64).unwrap_or(obj.string_param.moji_color);
            obj.string_param.shadow_color = script_args.get(5).and_then(as_i64).unwrap_or(obj.string_param.shadow_color);
            obj.string_param.shadow_mode = script_args.get(6).and_then(as_i64).unwrap_or(obj.string_param.shadow_mode);
        }
        if script_args.len() >= 8 {
            obj.string_param.fuchi_color = script_args.get(7).and_then(as_i64).unwrap_or(obj.string_param.fuchi_color);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_number != 0 && op == ctx.ids.obj_set_number {
        obj.number_value = script_args.get(0).and_then(as_i64).unwrap_or(obj.number_value);
        if matches!(obj.backend, ObjectBackend::Number { .. }) {
            update_number_backend(ctx, obj);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_number != 0 && op == ctx.ids.obj_get_number {
        ctx.stack.push(Value::Int(obj.number_value));
        return true;
    }
    if ctx.ids.obj_set_number_param != 0 && op == ctx.ids.obj_set_number_param {
        // SET_NUMBER_PARAM(keta_max, disp_zero, disp_sign, tumeru_sign, space_mod, space)
        obj.number_param.keta_max = script_args.get(0).and_then(as_i64).unwrap_or(obj.number_param.keta_max);
        obj.number_param.disp_zero = script_args.get(1).and_then(as_i64).unwrap_or(obj.number_param.disp_zero);
        obj.number_param.disp_sign = script_args.get(2).and_then(as_i64).unwrap_or(obj.number_param.disp_sign);
        obj.number_param.tumeru_sign = script_args.get(3).and_then(as_i64).unwrap_or(obj.number_param.tumeru_sign);
        obj.number_param.space_mod = script_args.get(4).and_then(as_i64).unwrap_or(obj.number_param.space_mod);
        obj.number_param.space = script_args.get(5).and_then(as_i64).unwrap_or(obj.number_param.space);
        if matches!(obj.backend, ObjectBackend::Number { .. }) {
            update_number_backend(ctx, obj);
        }
        push_ok(ctx, ret_form);
        return true;
    }



    // ---------------------------------------------------------------------
    // CREATE_* (ID-mapped)
    // ---------------------------------------------------------------------

    if ctx.ids.obj_create_number != 0 && op == ctx.ids.obj_create_number {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };

        object_clear_backend(ctx, obj, stage_idx, obj_u);

        // Reset compatibility state (matches C++ reinit(true)).
        obj.used = true;
        obj.object_type = 5;
        obj.number_value = 0;
        obj.string_param = Default::default();
        obj.number_param = Default::default();
        obj.weather_param = Default::default();
        obj.thumb_save_no = 0;
        obj.movie.reset();
        obj.emote = Default::default();
        obj.gan_file = None;
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();

        obj.file_name = Some(file.to_string());
        obj.string_value = None;

        let layer_id = ensure_rect_layer(ctx, st, stage_idx);
        let mut sprite_ids: Vec<SpriteId> = Vec::new();
        if let Some(layer) = ctx.layers.layer_mut(layer_id) {
            for _ in 0..16 {
                let sid = layer.create_sprite();
                if let Some(spr) = layer.sprite_mut(sid) {
                    spr.fit = SpriteFit::PixelRect;
                    spr.size_mode = SpriteSizeMode::Intrinsic;
                    spr.visible = false;
                    spr.image_id = None;
                }
                sprite_ids.push(sid);
            }
        }

        obj.backend = ObjectBackend::Number { layer_id, sprite_ids };

        // Optional parameters (al_id-based fallthrough): (disp, x, y)
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 {
                pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            obj.extra_int_props.insert(ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_x, pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0));
            }
            if ctx.ids.obj_y != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_y, pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0));
            }
        }
        if ctx.ids.obj_patno != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_patno, 0);
        }

        update_number_backend(ctx, obj);
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_weather != 0 && op == ctx.ids.obj_create_weather {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 4;
        obj.file_name = Some(file.to_string());
        obj.weather_param = Default::default();
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_mesh != 0 && op == ctx.ids.obj_create_mesh {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 6;
        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_billboard != 0 && op == ctx.ids.obj_create_billboard {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 7;
        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_save_thumb != 0 && op == ctx.ids.obj_create_save_thumb {
        let (pos, _named) = split_pos_named(script_args);
        let save_no = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 8;
        obj.thumb_save_no = save_no;
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();
        // Optional (disp, x, y) via al_id.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 { pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0) } else { 0 };
            obj.extra_int_props.insert(ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_x, pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0));
            }
            if ctx.ids.obj_y != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_y, pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0));
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_capture_thumb != 0 && op == ctx.ids.obj_create_capture_thumb {
        let (pos, _named) = split_pos_named(script_args);
        let save_no = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 11;
        obj.thumb_save_no = save_no;
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();
        // Optional (disp, x, y) via al_id.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 { pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0) } else { 0 };
            obj.extra_int_props.insert(ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_x, pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0));
            }
            if ctx.ids.obj_y != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_y, pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0));
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_capture != 0 && op == ctx.ids.obj_create_capture {
        let (pos, _named) = split_pos_named(script_args);
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 10;
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();
        // Optional parameters: (disp, x, y) via al_id with different indexing.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 { pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0) } else { 0 };
            obj.extra_int_props.insert(ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_x, pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0));
            }
            if ctx.ids.obj_y != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_y, pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0));
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    // Movie creation variants share one implementation.
    {
        let mut loop_flag = false;
        let mut wait_flag = false;
        let mut key_skip_flag = false;
        let mut matched = false;

        if ctx.ids.obj_create_movie != 0 && op == ctx.ids.obj_create_movie {
            matched = true;
        } else if ctx.ids.obj_create_movie_loop != 0 && op == ctx.ids.obj_create_movie_loop {
            matched = true;
            loop_flag = true;
        } else if ctx.ids.obj_create_movie_wait != 0 && op == ctx.ids.obj_create_movie_wait {
            matched = true;
            wait_flag = true;
        } else if ctx.ids.obj_create_movie_wait_key != 0 && op == ctx.ids.obj_create_movie_wait_key {
            matched = true;
            wait_flag = true;
            key_skip_flag = true;
        }

        if matched {
            let (pos, named) = split_pos_named(script_args);
            let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
                push_ok(ctx, ret_form);
                return true;
            };

            // Named args (id -> bool)
            let mut auto_free_flag = true;
            let mut real_time_flag = true;
            let mut ready_only_flag = false;
            for (id, v) in named {
                match id {
                    0 => auto_free_flag = v.as_i64().unwrap_or(0) != 0,
                    1 => real_time_flag = v.as_i64().unwrap_or(0) != 0,
                    2 => ready_only_flag = v.as_i64().unwrap_or(0) != 0,
                    _ => {}
                }
            }

            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.used = true;
            obj.object_type = 9;
            obj.file_name = Some(file.to_string());
            obj.string_value = None;
            obj.button.clear();
            obj.extra_int_props.clear();
            obj.extra_str_props.clear();
            obj.extra_events.clear();
            obj.rep_int_lists.clear();
            obj.rep_int_event_lists.clear();
            obj.cmd_int_args.clear();

            let total_ms = resolve_omv_path(&ctx.project_dir, file)
                .and_then(|p| omv_total_time_ms(&p));
            obj.movie.start(total_ms, loop_flag, auto_free_flag, real_time_flag, ready_only_flag);

            // Optional (disp, x, y) via al_id.
            let aid = al_id.unwrap_or(0);
            if ctx.ids.obj_disp != 0 {
                let disp_i = if aid >= 1 { pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0) } else { 0 };
                obj.extra_int_props.insert(ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
            }
            if aid >= 2 {
                if ctx.ids.obj_x != 0 {
                    obj.extra_int_props.insert(ctx.ids.obj_x, pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0));
                }
                if ctx.ids.obj_y != 0 {
                    obj.extra_int_props.insert(ctx.ids.obj_y, pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0));
                }
            }

            if wait_flag {
                // create_movie_wait(_key) calls wait_movie(key_skip_flag, key_skip_flag).
                ctx.wait.wait_object_movie(
                    ctx.ids.form_global_stage,
                    stage_idx,
                    obj_u,
                    key_skip_flag,
                    key_skip_flag,
                );
                // If the script expects an integer return (WAIT_KEY variant), the result
                // is produced when the wait completes. Do not push a placeholder now.
                if key_skip_flag && ret_form.unwrap_or(0) != 0 {
                    return true;
                }
            }

            push_ok(ctx, ret_form);
            return true;
        }
    }

    if ctx.ids.obj_create_emote != 0 && op == ctx.ids.obj_create_emote {
        let (pos, named) = split_pos_named(script_args);

        let width = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        let height = pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
        let Some(file) = pos.get(2).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };

        let mut rep_x: i64 = 0;
        let mut rep_y: i64 = 0;
        for (id, v) in named {
            let iv = v.as_i64().unwrap_or(0);
            match id {
                0 => rep_x = iv,
                1 => rep_y = iv,
                _ => {}
            }
        }

        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 12;
        obj.emote.width = width;
        obj.emote.height = height;
        obj.emote.file_name = Some(file.to_string());
        obj.emote.rep_x = rep_x;
        obj.emote.rep_y = rep_y;

        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.button.clear();
        obj.extra_int_props.clear();
        obj.extra_str_props.clear();
        obj.extra_events.clear();
        obj.rep_int_lists.clear();
        obj.rep_int_event_lists.clear();
        obj.cmd_int_args.clear();

        // Optional (disp, x, y) via al_id.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 { pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0) } else { 0 };
            obj.extra_int_props.insert(ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_x, pos.get(4).and_then(|v| v.as_i64()).unwrap_or(0));
            }
            if ctx.ids.obj_y != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_y, pos.get(5).and_then(|v| v.as_i64()).unwrap_or(0));
            }
        }

        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // WEATHER params
    // ---------------------------------------------------------------------

    if ctx.ids.obj_set_weather_param_type_a != 0 && op == ctx.ids.obj_set_weather_param_type_a {
        let (_pos, named) = split_pos_named(script_args);
        let mut wp = ObjectWeatherParam {
            weather_type: 1,
            cnt: 0,
            pat_mode: 0,
            pat_no_00: 0,
            pat_no_01: 0,
            pat_time: 0,
            move_time_x: 0,
            move_time_y: 0,
            sin_time_x: 0,
            sin_power_x: 0,
            sin_time_y: 0,
            sin_power_y: 0,
            center_x: 0,
            center_y: 0,
            appear_range: 0,
            move_time: 0,
            center_rotate: 0,
            zoom_min: 0,
            zoom_max: 0,
            scale_x: TNM_SCALE_UNIT,
            scale_y: TNM_SCALE_UNIT,
            active_time: 45000,
            real_time_flag: false,
        };

        for (id, v) in named {
            let iv = v.as_i64().unwrap_or(0);
            match id {
                0 => wp.cnt = iv,
                1 => wp.pat_mode = iv,
                2 => wp.pat_no_00 = iv,
                3 => wp.pat_no_01 = iv,
                4 => wp.pat_time = iv,
                5 => wp.move_time_x = iv,
                6 => wp.move_time_y = iv,
                7 => wp.sin_time_x = iv,
                8 => wp.sin_power_x = iv,
                9 => wp.sin_time_y = iv,
                10 => wp.sin_power_y = iv,
                11 => wp.real_time_flag = iv != 0,
                12 => wp.scale_x = iv,
                13 => wp.scale_y = iv,
                14 => wp.active_time = iv,
                _ => {}
            }
        }

        obj.weather_param = wp;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_weather_param_type_b != 0 && op == ctx.ids.obj_set_weather_param_type_b {
        let (_pos, named) = split_pos_named(script_args);
        let mut wp = ObjectWeatherParam {
            weather_type: 2,
            cnt: 0,
            pat_mode: 0,
            pat_no_00: 0,
            pat_no_01: 0,
            pat_time: 0,
            move_time_x: 0,
            move_time_y: 0,
            sin_time_x: 0,
            sin_power_x: 0,
            sin_time_y: 0,
            sin_power_y: 0,
            center_x: 0,
            center_y: 0,
            appear_range: 100,
            move_time: 1000,
            center_rotate: 0,
            zoom_min: TNM_SCALE_UNIT,
            zoom_max: TNM_SCALE_UNIT,
            scale_x: TNM_SCALE_UNIT,
            scale_y: TNM_SCALE_UNIT,
            active_time: 0,
            real_time_flag: false,
        };

        for (id, v) in named {
            let iv = v.as_i64().unwrap_or(0);
            match id {
                0 => wp.cnt = iv,
                1 => wp.pat_mode = iv,
                2 => wp.pat_no_00 = iv,
                3 => wp.pat_no_01 = iv,
                4 => wp.pat_time = iv,
                5 => wp.move_time = iv,
                6 => {},
                7 => wp.sin_time_x = iv,
                8 => wp.sin_power_x = iv,
                9 => wp.sin_time_y = iv,
                10 => wp.sin_power_y = iv,
                11 => wp.center_x = iv,
                12 => wp.center_y = iv,
                13 => wp.appear_range = iv,
                14 => wp.zoom_min = iv,
                15 => wp.zoom_max = iv,
                16 => wp.center_rotate = iv,
                17 => wp.real_time_flag = iv != 0,
                18 => wp.scale_x = iv,
                19 => wp.scale_y = iv,
                _ => {}
            }
        }

        obj.weather_param = wp;
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // MOVIE ops
    // ---------------------------------------------------------------------

    if ctx.ids.obj_pause_movie != 0 && op == ctx.ids.obj_pause_movie {
        obj.movie.pause_flag = true;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_resume_movie != 0 && op == ctx.ids.obj_resume_movie {
        obj.movie.pause_flag = false;
        // If a movie was created in ready-only mode, resume starts playback.
        obj.movie.playing = true;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_seek_movie != 0 && op == ctx.ids.obj_seek_movie {
        let t = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as u64;
        obj.movie.seek(t);
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_movie_seek_time != 0 && op == ctx.ids.obj_get_movie_seek_time {
        ctx.stack.push(Value::Int(obj.movie.get_seek_time() as i64));
        return true;
    }
    if ctx.ids.obj_check_movie != 0 && op == ctx.ids.obj_check_movie {
        ctx.stack.push(Value::Int(if obj.movie.check_movie() { 1 } else { 0 }));
        return true;
    }
    if ctx.ids.obj_wait_movie != 0 && op == ctx.ids.obj_wait_movie {
        if obj.movie.check_movie() {
            ctx.wait.wait_object_movie(ctx.ids.form_global_stage, stage_idx, obj_u, false, false);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_wait_movie_key != 0 && op == ctx.ids.obj_wait_movie_key {
        if obj.movie.check_movie() {
            // wait_movie(true, true)
            ctx.wait.wait_object_movie(ctx.ids.form_global_stage, stage_idx, obj_u, true, true);
            if ret_form.unwrap_or(0) != 0 {
                return true;
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_end_movie_loop != 0 && op == ctx.ids.obj_end_movie_loop {
        obj.movie.loop_flag = false;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_set_movie_auto_free != 0 && op == ctx.ids.obj_set_movie_auto_free {
        obj.movie.auto_free_flag = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // BUTTON ops (ID-mapped)
    // ---------------------------------------------------------------------

    if ctx.ids.obj_clear_button != 0 && op == ctx.ids.obj_clear_button {
        obj.button.clear();
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button != 0 && op == ctx.ids.obj_set_button {
        let (pos, _named) = split_pos_named(script_args);
        let ints = [
            pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0),
            pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0),
            pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
            pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0),
        ];
        let mut button_no = 0i64;
        let mut group_no = 0i64;
        let mut action_no = 0i64;
        let mut se_no = 0i64;
        match al_id.unwrap_or(0) {
            2 => {
                button_no = ints[0];
                group_no = ints[1];
                action_no = ints[2];
                se_no = ints[3];
            }
            1 => {
                button_no = ints[0];
                group_no = ints[1];
            }
            _ => {
                button_no = ints[0];
            }
        }
        obj.button.enabled = true;
        obj.button.button_no = button_no;
        obj.button.group_no = group_no;
        obj.button.action_no = action_no;
        obj.button.se_no = se_no;
        obj.button.hit = false;
        obj.button.pushed = false;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button_group != 0 && op == ctx.ids.obj_set_button_group {
        // al_id==0: int group_no, al_id==1: element group
        if al_id == Some(1) {
            if let Some(Value::Element(e)) = script_args.get(0) {
                if let Some(StageTarget::ChildItemOp { idx: gidx, .. }) = parse_target(ctx, e) {
                    obj.button.group_idx_override = Some(gidx.max(0) as usize);
                }
            }
        } else {
            let g = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
            obj.button.group_no = g;
            obj.button.group_idx_override = None;
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button_pushkeep != 0 && op == ctx.ids.obj_set_button_pushkeep {
        obj.button.push_keep = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_button_pushkeep != 0 && op == ctx.ids.obj_get_button_pushkeep {
        ctx.stack.push(Value::Int(if obj.button.push_keep { 1 } else { 0 }));
        return true;
    }

    if ctx.ids.obj_set_button_alpha_test != 0 && op == ctx.ids.obj_set_button_alpha_test {
        obj.button.alpha_test = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_button_alpha_test != 0 && op == ctx.ids.obj_get_button_alpha_test {
        ctx.stack.push(Value::Int(if obj.button.alpha_test { 1 } else { 0 }));
        return true;
    }

    if ctx.ids.obj_set_button_state_normal != 0 && op == ctx.ids.obj_set_button_state_normal {
        obj.button.state = TNM_BTN_STATE_NORMAL;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_set_button_state_select != 0 && op == ctx.ids.obj_set_button_state_select {
        obj.button.state = TNM_BTN_STATE_SELECT;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_set_button_state_disable != 0 && op == ctx.ids.obj_set_button_state_disable {
        obj.button.state = TNM_BTN_STATE_DISABLE;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_get_button_state != 0 && op == ctx.ids.obj_get_button_state {
        ctx.stack.push(Value::Int(obj.button.state));
        return true;
    }

    if ctx.ids.obj_get_button_hit_state != 0 && op == ctx.ids.obj_get_button_hit_state {
        ctx.stack.push(Value::Int(if obj.button.hit { TNM_BTN_STATE_HIT } else { TNM_BTN_STATE_NORMAL }));
        return true;
    }

    if ctx.ids.obj_get_button_real_state != 0 && op == ctx.ids.obj_get_button_real_state {
        // Best-effort: incorporate group selection.
        let mut stt = obj.button.state;
        if stt != TNM_BTN_STATE_SELECT && stt != TNM_BTN_STATE_DISABLE {
            if let Some(gidx) = obj.button.group_idx() {
                if let Some(gl) = st.group_lists.get(&stage_idx).and_then(|v| v.get(gidx)) {
                    if gl.decided_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_PUSH;
                    } else if gl.pushed_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_PUSH;
                    } else if gl.hit_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_HIT;
                    }
                }
            } else if obj.button.pushed {
                stt = TNM_BTN_STATE_PUSH;
            } else if obj.button.hit {
                stt = TNM_BTN_STATE_HIT;
            }
        }
        ctx.stack.push(Value::Int(stt));
        return true;
    }

    if ctx.ids.obj_set_button_call != 0 && op == ctx.ids.obj_set_button_call {
        // uses current scene name + cmd_name argument.
        // This runtime does not track the current scene name yet, so we store an empty scn_name.
        let cmd = script_args.get(0).and_then(|v| v.as_str()).unwrap_or("");
        obj.button.decided_action_scn_name.clear();
        obj.button.decided_action_cmd_name = cmd.to_string();
        obj.button.decided_action_z_no = -1;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_clear_button_call != 0 && op == ctx.ids.obj_clear_button_call {
        obj.button.decided_action_scn_name.clear();
        obj.button.decided_action_cmd_name.clear();
        obj.button.decided_action_z_no = -1;
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // FRAME_ACTION / GAN
    // ---------------------------------------------------------------------

    if ctx.ids.obj_load_gan != 0 && op == ctx.ids.obj_load_gan {
        let Some(name) = script_args.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        obj.gan_file = Some(name.to_string());
        push_ok(ctx, ret_form);
        return true;
    }

    // Multi-arg setters.
    if ctx.ids.obj_set_pos != 0 && op == ctx.ids.obj_set_pos {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        match obj.backend {
            ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                    if let Some(spr) = layer.sprite_mut(sprite_id) {
                        spr.x = x as i32;
                        spr.y = y as i32;
                    }
                }
            }
            ObjectBackend::Gfx => {
				let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
				let _ = gfx.object_set_pos(images, layers, stage_idx, obj_u as i64, x, y);
                if let Some(zv) = z {
                    let _ = ctx.gfx.object_set_z(stage_idx, obj_u as i64, zv);
                }
            }
            _ => {
                obj.extra_int_props.insert(ctx.ids.obj_x, x);
                obj.extra_int_props.insert(ctx.ids.obj_y, y);
                if let Some(zv) = z {
                    obj.extra_int_props.insert(ctx.ids.obj_z, zv);
                }
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_center != 0 && op == ctx.ids.obj_set_center {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_center_x != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_center_x, x);
        }
        if ctx.ids.obj_center_y != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_center_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_center_z != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_center_z, zv);
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_center_rep != 0 && op == ctx.ids.obj_set_center_rep {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_center_rep_x != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_center_rep_x, x);
        }
        if ctx.ids.obj_center_rep_y != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_center_rep_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_center_rep_z != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_center_rep_z, zv);
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_scale != 0 && op == ctx.ids.obj_set_scale {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_scale_x != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_scale_x, x);
        }
        if ctx.ids.obj_scale_y != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_scale_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_scale_z != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_scale_z, zv);
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_rotate != 0 && op == ctx.ids.obj_set_rotate {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_rotate_x != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_rotate_x, x);
        }
        if ctx.ids.obj_rotate_y != 0 {
            obj.extra_int_props.insert(ctx.ids.obj_rotate_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_rotate_z != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_rotate_z, zv);
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_clip != 0 && op == ctx.ids.obj_set_clip {
        // (use, left, top, right, bottom)
        if script_args.len() >= 5 {
            if ctx.ids.obj_clip_use != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_clip_use, script_args.get(0).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_clip_left != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_clip_left, script_args.get(1).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_clip_top != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_clip_top, script_args.get(2).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_clip_right != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_clip_right, script_args.get(3).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_clip_bottom != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_clip_bottom, script_args.get(4).and_then(as_i64).unwrap_or(0));
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_src_clip != 0 && op == ctx.ids.obj_set_src_clip {
        // (use, left, top, right, bottom)
        if script_args.len() >= 5 {
            if ctx.ids.obj_src_clip_use != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_src_clip_use, script_args.get(0).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_src_clip_left != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_src_clip_left, script_args.get(1).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_src_clip_top != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_src_clip_top, script_args.get(2).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_src_clip_right != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_src_clip_right, script_args.get(3).and_then(as_i64).unwrap_or(0));
            }
            if ctx.ids.obj_src_clip_bottom != 0 {
                obj.extra_int_props.insert(ctx.ids.obj_src_clip_bottom, script_args.get(4).and_then(as_i64).unwrap_or(0));
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    // Simple int properties that do not currently affect the renderer.
    {
        let simple_ids = [
            ctx.ids.obj_world,
            ctx.ids.obj_center_x,
            ctx.ids.obj_center_y,
            ctx.ids.obj_center_z,
            ctx.ids.obj_center_rep_x,
            ctx.ids.obj_center_rep_y,
            ctx.ids.obj_center_rep_z,
            ctx.ids.obj_scale_x,
            ctx.ids.obj_scale_y,
            ctx.ids.obj_scale_z,
            ctx.ids.obj_rotate_x,
            ctx.ids.obj_rotate_y,
            ctx.ids.obj_rotate_z,
            ctx.ids.obj_clip_use,
            ctx.ids.obj_clip_left,
            ctx.ids.obj_clip_top,
            ctx.ids.obj_clip_right,
            ctx.ids.obj_clip_bottom,
            ctx.ids.obj_src_clip_use,
            ctx.ids.obj_src_clip_left,
            ctx.ids.obj_src_clip_top,
            ctx.ids.obj_src_clip_right,
            ctx.ids.obj_src_clip_bottom,
            ctx.ids.obj_tr,
            ctx.ids.obj_mono,
            ctx.ids.obj_reverse,
            ctx.ids.obj_bright,
            ctx.ids.obj_dark,
            ctx.ids.obj_color_r,
            ctx.ids.obj_color_g,
            ctx.ids.obj_color_b,
            ctx.ids.obj_color_rate,
            ctx.ids.obj_color_add_r,
            ctx.ids.obj_color_add_g,
            ctx.ids.obj_color_add_b,
            ctx.ids.obj_mask_no,
            ctx.ids.obj_tonecurve_no,
            ctx.ids.obj_culling,
            ctx.ids.obj_alpha_test,
            ctx.ids.obj_alpha_blend,
            ctx.ids.obj_blend,
            ctx.ids.obj_light_no,
            ctx.ids.obj_fog_use,
        ];

        if simple_ids.iter().any(|&id| id != 0 && op == id) {
            let set_v = rhs.and_then(as_i64).or_else(|| {
                if ret_form.is_some() && al_id == Some(1) && script_args.len() == 1 {
                    script_args.get(0).and_then(as_i64)
                } else {
                    None
                }
            });
            if let Some(v) = set_v {
                obj.extra_int_props.insert(op, v);
                if ctx.ids.obj_alpha_test != 0 && op == ctx.ids.obj_alpha_test {
                    obj.button.alpha_test = v != 0;
                }
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(*obj.extra_int_props.get(&op).unwrap_or(&0)));
            }
            return true;
        }
    }

    // Query helpers.
    if ctx.ids.obj_get_pat_cnt != 0 && op == ctx.ids.obj_get_pat_cnt {
        // GET_PAT_CNT returns the available pattern count.
        let mut cnt = 0i64;
        if let Some(name) = obj.file_name.as_deref() {
            if let Ok((path, pct)) = crate::resource::find_g00_image(ctx.images.project_dir(), name) {
                match pct {
                    crate::resource::PctType::G00 => {
                        if let Ok(bytes) = std::fs::read(&path) {
                            if let Ok(decoded) = crate::assets::g00::decode_g00(&bytes) {
                                cnt = decoded.frames.len() as i64;
                            }
                        }
                    }
                    _ => {
                        cnt = 1;
                    }
                }
            }
        }
        ctx.stack.push(Value::Int(cnt));
        return true;
    }

    if (ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x)
        || (ctx.ids.obj_get_size_y != 0 && op == ctx.ids.obj_get_size_y)
        || (ctx.ids.obj_get_size_z != 0 && op == ctx.ids.obj_get_size_z)
    {
        // GET_SIZE_X/Y/Z(pat=0 or arg0)
        let pat = if al_id == Some(1) {
            script_args.get(0).and_then(as_i64).unwrap_or(0).max(0) as usize
        } else {
            0usize
        };

        let mut sx: i64 = 0;
        let mut sy: i64 = 0;
        let sz: i64 = 0;

        match obj.backend {
            ObjectBackend::Rect { width, height, .. } => {
                sx = width as i64;
                sy = height as i64;
            }
            _ => {
                if let Some(name) = obj.file_name.as_deref() {
                    if let Ok((path, _pct)) = crate::resource::find_g00_image(ctx.images.project_dir(), name) {
                        if let Ok(id) = ctx.images.load_file(&path, pat) {
                            if let Some(img) = ctx.images.get(id) {
                                sx = img.width as i64;
                                sy = img.height as i64;
                            }
                        }
                    }
                }
            }
        }

        if ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x {
            ctx.stack.push(Value::Int(sx));
        } else if ctx.ids.obj_get_size_y != 0 && op == ctx.ids.obj_get_size_y {
            ctx.stack.push(Value::Int(sy));
        } else {
            ctx.stack.push(Value::Int(sz));
        }
        return true;
    }

    if (ctx.ids.obj_get_pixel_color_r != 0 && op == ctx.ids.obj_get_pixel_color_r)
        || (ctx.ids.obj_get_pixel_color_g != 0 && op == ctx.ids.obj_get_pixel_color_g)
        || (ctx.ids.obj_get_pixel_color_b != 0 && op == ctx.ids.obj_get_pixel_color_b)
        || (ctx.ids.obj_get_pixel_color_a != 0 && op == ctx.ids.obj_get_pixel_color_a)
    {
        // GET_PIXEL_COLOR_* (x, y, pat=0 or arg2)
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let pat = if al_id == Some(1) {
            script_args.get(2).and_then(as_i64).unwrap_or(0).max(0) as usize
        } else {
            0usize
        };

        let mut out = 0i64;
        if x >= 0 && y >= 0 {
            if let Some(name) = obj.file_name.as_deref() {
                if let Ok((path, _pct)) = crate::resource::find_g00_image(ctx.images.project_dir(), name) {
                    if let Ok(id) = ctx.images.load_file(&path, pat) {
                        if let Some(img) = ctx.images.get(id) {
                            let xi = x as u32;
                            let yi = y as u32;
                            if xi < img.width && yi < img.height {
                                let idx = ((yi * img.width + xi) * 4) as usize;
                                if idx + 4 <= img.rgba.len() {
                                    let r = img.rgba[idx] as i64;
                                    let g = img.rgba[idx + 1] as i64;
                                    let b = img.rgba[idx + 2] as i64;
                                    let a = img.rgba[idx + 3] as i64;
                                    out = if ctx.ids.obj_get_pixel_color_r != 0 && op == ctx.ids.obj_get_pixel_color_r {
                                        r
                                    } else if ctx.ids.obj_get_pixel_color_g != 0 && op == ctx.ids.obj_get_pixel_color_g {
                                        g
                                    } else if ctx.ids.obj_get_pixel_color_b != 0 && op == ctx.ids.obj_get_pixel_color_b {
                                        b
                                    } else {
                                        a
                                    };
                                }
                            }
                        }
                    }
                }
            }
        }
        ctx.stack.push(Value::Int(out));
        return true;
    }

    // Learn + execute command-like object ops.
    let k = learn_object_op(st, op, script_args, ret_form, rhs, al_id);
    match k {
        ObjectOpKind::Init => {
            // INIT => reinit(true)
            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.button.clear();
            obj.extra_int_props.clear();
            obj.extra_str_props.clear();
            obj.extra_events.clear();
            obj.rep_int_lists.clear();
            obj.rep_int_event_lists.clear();
            obj.cmd_int_args.clear();
            obj.file_name = None;
            obj.string_value = None;
            obj.object_type = 0;
            obj.number_value = 0;
            obj.string_param = Default::default();
            obj.number_param = Default::default();
            obj.used = true;
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::Free => {
            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.button.clear();
            *obj = ObjectCompatState::default();
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::InitParam => {
            // INIT_PARAM => init_param(true)
            // We do not have the full def-param table yet; keep the existing object and
            // reset only parameter blocks we can represent.
            obj.button.clear();
            obj.string_param = Default::default();
            obj.number_param = Default::default();
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::ClearButton => {
            obj.button.clear();
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::SetButton => {
            // Should have been handled by the early SetButton path.
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::SetButtonGroup => {
            // Should have been handled by the early SetButtonGroup path.
            ctx.stack.push(Value::Int(0));
            true
        }

        ObjectOpKind::CreateRect => {
            if script_args.len() < 8 {
                push_ok(ctx, ret_form);
                return true;
            }
            let l = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let t = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let r = script_args.get(2).and_then(as_i64).unwrap_or(l);
            let b = script_args.get(3).and_then(as_i64).unwrap_or(t);

            let rr = script_args.get(4).and_then(as_i64).unwrap_or(0).clamp(0, 255) as u8;
            let gg = script_args.get(5).and_then(as_i64).unwrap_or(0).clamp(0, 255) as u8;
            let bb = script_args.get(6).and_then(as_i64).unwrap_or(0).clamp(0, 255) as u8;
            let aa = script_args.get(7).and_then(as_i64).unwrap_or(255).clamp(0, 255) as u8;

            // optional args depend on al_id; here we derive from argc.
            let disp = if script_args.len() >= 9 {
                script_args.get(8).and_then(as_i64).unwrap_or(0) != 0
            } else {
                false
            };
            let (x, y) = if script_args.len() >= 11 {
                (
                    script_args.get(9).and_then(as_i64).unwrap_or(0) as i32,
                    script_args.get(10).and_then(as_i64).unwrap_or(0) as i32,
                )
            } else {
                (0, 0)
            };

            let w = (l.abs_diff(r)).clamp(1, 32767) as u32;
            let h = (t.abs_diff(b)).clamp(1, 32767) as u32;

            // If we were a gfx object, clear it; for an existing rect backend we reuse the sprite.
            if matches!(obj.backend, ObjectBackend::Gfx) {
				let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
				let _ = gfx.object_clear(images, layers, stage_idx, obj_u as i64);
            }

            let layer_id = ensure_rect_layer(ctx, st, stage_idx);
            let sprite_id = match obj.backend {
                ObjectBackend::Rect { layer_id: lid, sprite_id: sid, .. } if lid == layer_id => sid,
                _ => {
                    let Some(sid) = ctx.layers.layer_mut(layer_id).map(|layer| layer.create_sprite()) else {
                        push_ok(ctx, ret_form);
                        return true;
                    };
                    sid
                }
            };

            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                if let Some(spr) = layer.sprite_mut(sprite_id) {
                    let img = ctx.images.solid_rgba((rr, gg, bb, aa));
                    spr.image_id = Some(img);
                    spr.fit = SpriteFit::PixelRect;
                    spr.size_mode = SpriteSizeMode::Explicit { width: w, height: h };
                    spr.visible = disp;
                    spr.x = x as i32;
                    spr.y = y as i32;
                }
            }

            obj.used = true;
            obj.backend = ObjectBackend::Rect { layer_id, sprite_id, width: w, height: h };
            obj.object_type = 1;
            obj.number_value = 0;
            obj.string_param = Default::default();
            obj.number_param = Default::default();
            obj.file_name = None;
            obj.string_value = None;
            obj.extra_int_props.clear();
            obj.extra_str_props.clear();
            obj.extra_events.clear();
            obj.rep_int_lists.clear();
            obj.rep_int_event_lists.clear();
            obj.cmd_int_args.clear();
            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::CreateString => {
            let Some(s0) = script_args.get(0).and_then(as_str) else {
                push_ok(ctx, ret_form);
                return true;
            };

            // optional disp and pos depend on al_id; derive from argc.
            let disp = if script_args.len() >= 2 {
                script_args.get(1).and_then(as_i64).unwrap_or(0) != 0
            } else {
                false
            };
            let (x, y) = if script_args.len() >= 4 {
                (
                    script_args.get(2).and_then(as_i64).unwrap_or(0),
                    script_args.get(3).and_then(as_i64).unwrap_or(0),
                )
            } else {
                (0, 0)
            };

            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.used = true;
            obj.backend = ObjectBackend::None;
            obj.object_type = 3;
            obj.number_value = 0;
            obj.file_name = None;
            obj.string_value = Some(s0.to_string());
            obj.extra_int_props.clear();
            obj.extra_str_props.clear();
            obj.extra_events.clear();
            obj.rep_int_lists.clear();
            obj.rep_int_event_lists.clear();
            obj.cmd_int_args.clear();

            // Preserve base props for later GET via obj_disp/obj_x/obj_y if scripts query them.
            obj.extra_int_props.insert(ctx.ids.obj_disp, if disp { 1 } else { 0 });
            obj.extra_int_props.insert(ctx.ids.obj_x, x);
            obj.extra_int_props.insert(ctx.ids.obj_y, y);

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::CreatePct => {
            let Some(file) = script_args.get(0).and_then(as_str) else {
                push_ok(ctx, ret_form);
                return true;
            };

            // optional args depend on al_id; derive from argc.
            let disp = if script_args.len() >= 2 {
                script_args.get(1).and_then(as_i64).unwrap_or(0) != 0
            } else {
                false
            };
            let (x, y) = if script_args.len() >= 4 {
                (
                    script_args.get(2).and_then(as_i64).unwrap_or(0),
                    script_args.get(3).and_then(as_i64).unwrap_or(0),
                )
            } else {
                (0, 0)
            };
            let patno = if script_args.len() >= 5 {
                script_args.get(4).and_then(as_i64).unwrap_or(0)
            } else {
                0
            };

            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.extra_int_props.clear();
            obj.extra_str_props.clear();
            obj.extra_events.clear();
            obj.rep_int_lists.clear();
            obj.rep_int_event_lists.clear();
            obj.cmd_int_args.clear();

			{
				let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
				let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, file, disp as i64, x, y, patno);
			}
            obj.used = true;
            obj.backend = ObjectBackend::Gfx;
            obj.object_type = 2;
            obj.number_value = 0;
            obj.string_param = Default::default();
            obj.number_param = Default::default();
            obj.file_name = Some(file.to_string());
            obj.string_value = None;
            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetPos => {
            let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let z = script_args.get(2).and_then(as_i64);

            obj.cmd_int_args
                .insert(op, vec![x, y].into_iter().chain(z).collect());

            match obj.backend {
                ObjectBackend::Rect { layer_id, sprite_id, .. } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.x = x as i32;
                            spr.y = y as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
					let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
					let _ = gfx.object_set_pos(images, layers, stage_idx, obj_u as i64, x, y);
                    if let Some(zv) = z {
                        let _ = ctx.gfx.object_set_z(stage_idx, obj_u as i64, zv);
                    }
                }
                _ => {
                    obj.extra_int_props.insert(ctx.ids.obj_x, x);
                    obj.extra_int_props.insert(ctx.ids.obj_y, y);
                    if let Some(zv) = z {
                        obj.extra_int_props.insert(op, zv);
                    }
                }
            }

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetCenter | ObjectOpKind::SetScale | ObjectOpKind::SetRotate => {
            // These affect rendering in the original engine. The current renderer does not
            // support pivot/scale/rotation, so we keep them as state for script compatibility.
            let mut v = Vec::new();
            for a in script_args.iter().take(3) {
                v.push(a.as_i64().unwrap_or(0));
            }
            obj.cmd_int_args.insert(op, v);
            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetClip | ObjectOpKind::SetSrcClip => {
            // Clip rectangles are stored for compatibility; rendering-side clipping is not implemented.
            let mut v = Vec::new();
            for a in script_args.iter().take(4) {
                v.push(a.as_i64().unwrap_or(0));
            }
            obj.cmd_int_args.insert(op, v);
            push_ok(ctx, ret_form);
            true
        }
        _ => {
            // Generic property storage fallback (supports both int and string).
            if let Some(Value::Int(v)) = rhs {
                obj.extra_int_props.insert(op, *v);
                ctx.stack.push(Value::Int(0));
                return true;
            }
            if let Some(Value::Str(v)) = rhs {
                obj.extra_str_props.insert(op, v.clone());
                st.object_str_ops.insert(op);
                ctx.stack.push(Value::Int(0));
                return true;
            }

            // Property-get fallback.
            if rhs.is_none() && ret_form.is_none() {
                if st.object_str_ops.contains(&op) {
                    if let Some(sv) = obj.extra_str_props.get(&op) {
                        ctx.stack.push(Value::Str(sv.clone()));
                    } else if let Some(f) = &obj.file_name {
                        ctx.stack.push(Value::Str(f.clone()));
                    } else if let Some(t) = &obj.string_value {
                        ctx.stack.push(Value::Str(t.clone()));
                    } else {
                        ctx.stack.push(Value::Str(String::new()));
                    }
                } else {
                    ctx.stack.push(Value::Int(*obj.extra_int_props.get(&op).unwrap_or(&0)));
                }
                return true;
            }

            // Unknown command: default return value (or 0).
            push_ok(ctx, ret_form);
            true
        }
    }
}

fn learn_group_list_op(st: &mut StageFormState, op: i32, argc: usize) -> GroupListOpKind {
    if let Some(&k) = st.group_list_op_map.get(&op) {
        return k;
    }
    let k = if argc == 1 && !st.group_list_op_map.values().any(|&v| v == GroupListOpKind::Alloc) {
        GroupListOpKind::Alloc
    } else if argc == 0 && !st.group_list_op_map.values().any(|&v| v == GroupListOpKind::Free) {
        GroupListOpKind::Free
    } else {
        GroupListOpKind::Unknown
    };
    st.group_list_op_map.insert(op, k);
    k
}

fn learn_group_op(st: &mut StageFormState, op: i32, argc: usize, ret_form: Option<i64>, rhs: Option<&Value>, al_id: Option<i64>) -> GroupOpKind {
    if let Some(&k) = st.group_op_map.get(&op) {
        return k;
    }

    // Property-style ops use al_id (0=get, 1=set) and may have rhs.
    if al_id.is_some() && rhs.is_some() {
        // Prefer mapping the first 3 property ops as ORDER/LAYER/CANCEL_PRIORITY.
        let used = st.group_op_map.values().copied().collect::<Vec<_>>();
        let k = if !used.contains(&GroupOpKind::Order) {
            GroupOpKind::Order
        } else if !used.contains(&GroupOpKind::Layer) {
            GroupOpKind::Layer
        } else if !used.contains(&GroupOpKind::CancelPriority) {
            GroupOpKind::CancelPriority
        } else {
            GroupOpKind::Unknown
        };
        st.group_op_map.insert(op, k);
        return k;
    }

    // Getter ops: no args, return int (ret_form absent for call_property; present for call_command).
    let returns_int = match ret_form {
        Some(rf) => rf != 0 && rf != 2,
        None => true,
    };

    if argc == 0 && returns_int {
        let used = st.group_op_map.values().copied().collect::<Vec<_>>();
        let k = if !used.contains(&GroupOpKind::GetResult) {
            GroupOpKind::GetResult
        } else if !used.contains(&GroupOpKind::GetResultButtonNo) {
            GroupOpKind::GetResultButtonNo
        } else if !used.contains(&GroupOpKind::GetDecidedNo) {
            GroupOpKind::GetDecidedNo
        } else if !used.contains(&GroupOpKind::GetPushedNo) {
            GroupOpKind::GetPushedNo
        } else if !used.contains(&GroupOpKind::GetHitNo) {
            GroupOpKind::GetHitNo
        } else {
            GroupOpKind::Unknown
        };
        st.group_op_map.insert(op, k);
        return k;
    }

    // Selection ops:
    if argc == 1 && !st.has_group_op(GroupOpKind::SelCancel) {
        st.group_op_map.insert(op, GroupOpKind::SelCancel);
        return GroupOpKind::SelCancel;
    }

    if argc == 0 {
        // Ambiguous (INIT/SEL/START/END). We bias toward SEL (blocking) to avoid deadlocks.
        if !st.has_group_op(GroupOpKind::Sel) {
            st.group_op_map.insert(op, GroupOpKind::Sel);
            return GroupOpKind::Sel;
        }
        if !st.has_group_op(GroupOpKind::End) {
            st.group_op_map.insert(op, GroupOpKind::End);
            return GroupOpKind::End;
        }
        if !st.has_group_op(GroupOpKind::Init) {
            st.group_op_map.insert(op, GroupOpKind::Init);
            return GroupOpKind::Init;
        }
        if !st.has_group_op(GroupOpKind::Start) {
            st.group_op_map.insert(op, GroupOpKind::Start);
            return GroupOpKind::Start;
        }
    }

    st.group_op_map.insert(op, GroupOpKind::Unknown);
    GroupOpKind::Unknown
}

fn dispatch_group_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let k = learn_group_list_op(st, op, script_args.len());
    match k {
        GroupListOpKind::Alloc => {
            let cnt = script_args.iter().find_map(as_i64).unwrap_or(0).max(0) as usize;
            st.ensure_group_list(stage_idx, cnt);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            true
        }
        GroupListOpKind::Free => {
            st.clear_group_list(stage_idx);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            true
        }
        GroupListOpKind::Unknown => false,
    }
}

fn dispatch_group_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    group_idx: usize,
    op: i32,
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    ensure_group(ctx, st, stage_idx, group_idx);
    let k = learn_group_op(st, op, script_args.len(), ret_form, rhs, al_id);

    let list = st.group_lists.get_mut(&stage_idx).unwrap();
    let g = &mut list[group_idx];

    match k {
        GroupOpKind::Order => {
            if let Some(v) = rhs.and_then(as_i64) {
                g.order = v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(g.order));
            }
            true
        }
        GroupOpKind::Layer => {
            if let Some(v) = rhs.and_then(as_i64) {
                g.layer = v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(g.layer));
            }
            true
        }
        GroupOpKind::CancelPriority => {
            if let Some(v) = rhs.and_then(as_i64) {
                g.cancel_priority = v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(g.cancel_priority));
            }
            true
        }
        GroupOpKind::Init => {
            g.init_sel();
            g.wait_flag = false;
            g.started = false;
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::Start | GroupOpKind::StartCancel => {
            g.init_sel();
            g.cancel_flag = (k == GroupOpKind::StartCancel);
            g.start();
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::Sel | GroupOpKind::SelCancel => {
            // Mirror group_sel(_cancel) behavior:
            // - reset selection state
            // - start selection
            // - set wait flag and focus so Enter/Esc can drive it
            g.init_sel();
            g.cancel_flag = (k == GroupOpKind::SelCancel);
            if k == GroupOpKind::SelCancel {
                g.cancel_se_no = script_args.iter().find_map(as_i64).unwrap_or(-1);
            }
            g.wait_flag = true;
            g.start();

            // Focus this group for bring-up key mapping (see runtime::CommandContext::on_key_down).
            ctx.globals.focused_stage_group = Some((ctx.ids.form_global_stage, stage_idx, group_idx));

            // Block VM until a key is pressed.
            ctx.wait.wait_key();

            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::End => {
            g.end();
            if ctx.globals.focused_stage_group == Some((ctx.ids.form_global_stage, stage_idx, group_idx)) {
                ctx.globals.focused_stage_group = None;
            }
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::GetHitNo => {
            ctx.stack.push(Value::Int(g.hit_button_no));
            true
        }
        GroupOpKind::GetPushedNo => {
            ctx.stack.push(Value::Int(g.pushed_button_no));
            true
        }
        GroupOpKind::GetDecidedNo => {
            ctx.stack.push(Value::Int(g.decided_button_no));
            true
        }
        GroupOpKind::GetResult => {
            ctx.stack.push(Value::Int(g.result));
            true
        }
        GroupOpKind::GetResultButtonNo => {
            ctx.stack.push(Value::Int(g.result_button_no));
            true
        }
        GroupOpKind::Unknown => false,
    }
}

fn learn_mwnd_list_op(st: &mut StageFormState, op: i32, argc: usize) -> MwndListOpKind {
    if let Some(&k) = st.mwnd_list_op_map.get(&op) {
        return k;
    }
    let k = if argc == 0 && !st.mwnd_list_op_map.values().any(|&v| v == MwndListOpKind::CloseAll) {
        MwndListOpKind::CloseAll
    } else {
        MwndListOpKind::Unknown
    };
    st.mwnd_list_op_map.insert(op, k);
    k
}

fn learn_mwnd_op(st: &mut StageFormState, op: i32, script_args: &[Value], ret_form: Option<i64>) -> MwndOpKind {
    // This function is kept for compatibility but now defaults to Unknown.
    // MWND op kinds are learned in dispatch using runtime state.
    let _ = (st, op, script_args, ret_form);
    MwndOpKind::Unknown
}

fn dispatch_mwnd_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let k = learn_mwnd_list_op(st, op, script_args.len());
    match k {
        MwndListOpKind::CloseAll => {
            st.close_all_mwnd(stage_idx);
            // Commit the current backlog entry before clearing UI state.
            msgbk_next(ctx);
            ctx.ui.clear_message();
            ctx.ui.clear_name();
            ctx.ui.show_message_bg(false);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            true
        }
        MwndListOpKind::Unknown => false,
    }
}

fn dispatch_mwnd_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    mwnd_idx: usize,
    op: i32,
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    ensure_mwnd(ctx, st, stage_idx, mwnd_idx);
    let m_snapshot = {
        let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
        list[mwnd_idx].clone()
    };

    // Decide and cache MWND op kind on first sight.
    let k = if let Some(&k) = st.mwnd_op_map.get(&op) {
        k
    } else {
        let mut decided = MwndOpKind::Unknown;

        // No-arg string-return getters (name / waku file / filter file).
        if script_args.is_empty() && rhs.is_none() && matches!(ret_form, Some(2)) {
            if st.has_mwnd_op(MwndOpKind::SetName) && !st.has_mwnd_op(MwndOpKind::GetName) {
                decided = MwndOpKind::GetName;
            } else if !m_snapshot.waku_file.is_empty() && !st.has_mwnd_op(MwndOpKind::GetWakuFile) {
                decided = MwndOpKind::GetWakuFile;
            } else if !m_snapshot.filter_file.is_empty() && !st.has_mwnd_op(MwndOpKind::GetFilterFile) {
                decided = MwndOpKind::GetFilterFile;
            } else if !st.has_mwnd_op(MwndOpKind::GetName) {
                decided = MwndOpKind::GetName;
            } else if !st.has_mwnd_op(MwndOpKind::GetWakuFile) {
                decided = MwndOpKind::GetWakuFile;
            } else if !st.has_mwnd_op(MwndOpKind::GetFilterFile) {
                decided = MwndOpKind::GetFilterFile;
            }
        }

        // String ops (message / name / file setters).
        if decided == MwndOpKind::Unknown {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()));
            if let Some(s) = s {
                // In the original engine, add_msg-like calls return an overflow string.
                if matches!(ret_form, Some(2)) {
                    decided = MwndOpKind::AddMsg;
                } else if looks_like_path(s) {
                    if !st.has_mwnd_op(MwndOpKind::SetWakuFile) {
                        decided = MwndOpKind::SetWakuFile;
                    } else if !st.has_mwnd_op(MwndOpKind::SetFilterFile) {
                        decided = MwndOpKind::SetFilterFile;
                    } else {
                        decided = MwndOpKind::Print;
                    }
                } else if name_candidate(s) && ctx.ui.current_message.is_none() && !st.has_mwnd_op(MwndOpKind::SetName) {
                    decided = MwndOpKind::SetName;
                } else {
                    decided = MwndOpKind::Print;
                }
            }
        }

        // CheckOpen: no args, int return.
        if decided == MwndOpKind::Unknown {
            if script_args.is_empty() && rhs.is_none() && matches!(ret_form, Some(rf) if rf != 0 && rf != 2) {
                if !st.has_mwnd_op(MwndOpKind::CheckOpen) {
                    decided = MwndOpKind::CheckOpen;
                }
            }
        }

        // add_msg_check-like: one int arg, int return.
        if decided == MwndOpKind::Unknown {
            if script_args.len() == 1 && rhs.is_none() && matches!(ret_form, Some(rf) if rf != 0 && rf != 2) {
                if script_args[0].as_i64().is_some() && !st.has_mwnd_op(MwndOpKind::AddMsgCheck) {
                    decided = MwndOpKind::AddMsgCheck;
                }
            }
        }

        // No-arg void ops: Open/Close/Wait/Clear/Page/NL.
        if decided == MwndOpKind::Unknown {
            if script_args.is_empty() && rhs.is_none() && matches!(ret_form, Some(0) | None) {
                // Once the window is open (or UI bg is visible), scripts frequently issue
                // proceed-like ops (PP/R/PAGE/WAIT) even when the message buffer is empty.
                // Conservatively prefer Wait/Page/NL before structural ops to reduce
                // mis-classification.
                let msg_has_text = ctx
                    .ui
                    .current_message
                    .as_ref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                let open_context = m_snapshot.open || ctx.ui.msg_bg_visible;
                let has_activity = msg_has_text || ctx.ui.current_name.is_some() || m_snapshot.text_dirty;

                if open_context {
                    if !st.has_mwnd_op(MwndOpKind::WaitMsg) {
                        decided = MwndOpKind::WaitMsg;
                    } else if !st.has_mwnd_op(MwndOpKind::PageWait) {
                        decided = MwndOpKind::PageWait;
                    } else if !st.has_mwnd_op(MwndOpKind::NewLine) {
                        decided = MwndOpKind::NewLine;
                    } else if !st.has_mwnd_op(MwndOpKind::Clear) && has_activity {
                        // CLEAR is more plausible when there was activity.
                        decided = MwndOpKind::Clear;
                    } else if !st.has_mwnd_op(MwndOpKind::Close) {
                        decided = MwndOpKind::Close;
                    } else {
                        decided = MwndOpKind::WaitMsg;
                    }
                } else {
                    if st.has_mwnd_op(MwndOpKind::SetWakuFile) && !st.has_mwnd_op(MwndOpKind::InitWakuFile) {
                        decided = MwndOpKind::InitWakuFile;
                    } else if st.has_mwnd_op(MwndOpKind::SetFilterFile) && !st.has_mwnd_op(MwndOpKind::InitFilterFile) {
                        decided = MwndOpKind::InitFilterFile;
                    } else if st.has_mwnd_op(MwndOpKind::SetName) && !st.has_mwnd_op(MwndOpKind::ClearName) {
                        decided = MwndOpKind::ClearName;
                    } else if !st.has_mwnd_op(MwndOpKind::Open) {
                        decided = MwndOpKind::Open;
                    } else if !st.has_mwnd_op(MwndOpKind::WaitMsg) {
                        decided = MwndOpKind::WaitMsg;
                    } else if !st.has_mwnd_op(MwndOpKind::Clear) {
                        decided = MwndOpKind::Clear;
                    } else if !st.has_mwnd_op(MwndOpKind::PageWait) {
                        decided = MwndOpKind::PageWait;
                    } else if !st.has_mwnd_op(MwndOpKind::NewLine) {
                        decided = MwndOpKind::NewLine;
                    } else if !st.has_mwnd_op(MwndOpKind::Close) {
                        decided = MwndOpKind::Close;
                    }
                }
            }
        }

        // One-int-arg void ops are often wait variants.
        if decided == MwndOpKind::Unknown {
            if script_args.len() == 1 && rhs.is_none() && matches!(ret_form, Some(0) | None) {
                if script_args[0].as_i64().is_some() {
                    if !st.has_mwnd_op(MwndOpKind::WaitMsg) {
                        decided = MwndOpKind::WaitMsg;
                    } else if !st.has_mwnd_op(MwndOpKind::PageWait) {
                        decided = MwndOpKind::PageWait;
                    }
                }
            }
        }

        st.mwnd_op_map.insert(op, decided);
        decided
    };

        let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
    let m = &mut list[mwnd_idx];

match k {
        MwndOpKind::Open => {
            m.open = true;
            m.text_dirty = false;
            ensure_default_msg_bg(ctx);
            if !m.waku_file.is_empty() {
                try_set_ui_bg_from_name(ctx, &m.waku_file);
            }
            ctx.ui.show_message_bg(true);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Close => {
            m.open = false;
            m.text_dirty = false;
            msgbk_next(ctx);
            ctx.ui.clear_message();
            ctx.ui.clear_name();
            ctx.ui.show_message_bg(false);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::CheckOpen => {
            ctx.stack.push(Value::Int(if m.open { 1 } else { 0 }));
            true
        }
        MwndOpKind::Clear => {
            ctx.ui.clear_message();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NewLine => {
            ensure_default_msg_bg(ctx);
            ctx.ui.show_message_bg(true);
            ctx.ui.append_linebreak();
            m.text_dirty = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Print => {
            // Best-effort: pick the first string argument as message text.
            let msg = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            if !msg.is_empty() {
                ensure_default_msg_bg(ctx);
                ctx.ui.show_message_bg(true);
                ctx.ui.append_message(msg);
                msgbk_add_text(ctx, msg);
                m.text_dirty = true;
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::AddMsg => {
            let msg = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            if !msg.is_empty() {
                ensure_default_msg_bg(ctx);
                ctx.ui.show_message_bg(true);
                ctx.ui.append_message(msg);
                msgbk_add_text(ctx, msg);
                m.text_dirty = true;
            }
            // Original add_msg returns an overflow string; we do not simulate overflow.
            if matches!(ret_form, Some(2)) {
                ctx.stack.push(Value::Str(String::new()));
            } else {
                push_ok(ctx, ret_form);
            }
            true
        }
        MwndOpKind::AddMsgCheck => {
            // Original behavior depends on internal layout and remaining capacity.
            // Bring-up: always allow.
            ctx.stack.push(Value::Int(1));
            true
        }
        MwndOpKind::WaitMsg => {
            // Bring-up: wait for any key / mouse.
            ctx.ui.begin_wait_message();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::PageWait => {
            // Treat a page wait as a message boundary for backlog purposes.
            msgbk_next(ctx);
            ctx.ui.begin_wait_message();
            ctx.ui.request_clear_message_on_wait_end();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetName => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            ensure_default_msg_bg(ctx);
            ctx.ui.show_message_bg(true);
            ctx.ui.set_name(s.to_string());
            if !s.is_empty() {
                msgbk_add_name(ctx, s);
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::ClearName => {
            ctx.ui.clear_name();
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetName => {
            ctx.stack.push(Value::Str(ctx.ui.current_name.clone().unwrap_or_default()));
            true
        }
        MwndOpKind::InitWakuFile => {
            m.waku_file.clear();
            ensure_default_msg_bg(ctx);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetWakuFile => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            m.waku_file = s.to_string();
            if m.open {
                ensure_default_msg_bg(ctx);
                try_set_ui_bg_from_name(ctx, &m.waku_file);
                ctx.ui.show_message_bg(true);
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetWakuFile => {
            ctx.stack.push(Value::Str(m.waku_file.clone()));
            true
        }
        MwndOpKind::InitFilterFile => {
            m.filter_file.clear();
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetFilterFile => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            m.filter_file = s.to_string();
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetFilterFile => {
            ctx.stack.push(Value::Str(m.filter_file.clone()));
            true
        }
        MwndOpKind::Unknown => {
            // Best-effort generic property storage to keep scripts progressing.
            // Many MWND properties are int-valued (e.g., speed, flags, layout).
            if let Some(v) = rhs.and_then(|v| v.as_i64()) {
                m.props.insert(op, v);
                ctx.stack.push(Value::Int(0));
                return true;
            }

            if rhs.is_none() && script_args.len() == 1 {
                if let Some(v) = script_args[0].as_i64() {
                    if matches!(al_id, Some(1)) {
                        m.props.insert(op, v);
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                }
            }

            if rhs.is_none() {
                if let Some(rf) = ret_form {
                    if rf == 2 {
                        // String return: keep it empty.
                        ctx.stack.push(Value::Str(String::new()));
                        return true;
                    }
                    if rf != 0 {
                        ctx.stack.push(Value::Int(*m.props.get(&op).unwrap_or(&0)));
                        return true;
                    }
                }
            }

            false
        }
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = find_chain(args) else {
        return Ok(false);
    };

    // Optional command metadata exists only for call_command():
    //   [..., Element(chain), al_id, ret_form]
    let mut al_id: Option<i64> = None;
    let mut ret_form: Option<i64> = None;
    if chain_pos + 2 < args.len() {
        if let (Some(a), Some(r)) = (as_i64(&args[chain_pos + 1]), as_i64(&args[chain_pos + 2])) {
            al_id = Some(a);
            ret_form = Some(r);
        }
    }

    // Property-assign shape (call_property_assign):
    //   [op_id, al_id, rhs, Element(chain)]
    // Property-get shape (call_property):
    //   [op_id, Element(chain)]
    let rhs: Option<&Value> = if ret_form.is_none() && chain_pos >= 2 {
        if as_i64(&args[1]).is_some() {
            Some(&args[2])
        } else {
            None
        }
    } else {
        None
    };

    let Some(tgt) = parse_target(ctx, &chain) else {
        return Ok(false);
    };

    // Command args sit before Element(chain). The first value is the synthetic op_id
    // inserted by VM for form calls, and is not meaningful here.
    let script_args = if chain_pos >= 1 {
        &args[1..chain_pos]
    } else {
        &[][..]
    };

    match tgt {
        StageTarget::StageCount => {
            // Stage count: expose 3 logical stages (BG/CHR/FX).
            ctx.stack.push(Value::Int(3));
            return Ok(true);
        }
        StageTarget::StageOp { stage: _, op: _ } => {
            // CreateObject/CreateMwnd exist on the stage itself in the original engine.
            // Without a reliable id-map, we keep this as a stub that returns 0.
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                } else {
                    ctx.stack.push(Value::Int(0));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            return Ok(true);
        }
        StageTarget::ChildListOp { stage, child, op } => {
            let form_id = ctx.ids.form_global_stage;
            let stage_elm_object = ctx.ids.stage_elm_object;

            with_stage_state(ctx, form_id, |ctx, st| {
                // Route by learned kind (or try to learn).
                let kind = st.child_kind.get(&child).copied().unwrap_or(StageChildKind::Unknown);
                let mut handled = false;

                if (kind == StageChildKind::ObjectList || kind == StageChildKind::Unknown)
                    && child == stage_elm_object
                {
                    if dispatch_object_list_op(ctx, st, stage, op as i32, script_args, ret_form) {
                        st.child_kind.insert(child, StageChildKind::ObjectList);
                        handled = true;
                    }
                }

                if !handled && (kind == StageChildKind::GroupList || kind == StageChildKind::Unknown) {
                    if dispatch_group_list_op(ctx, st, stage, op as i32, script_args, ret_form) {
                        st.child_kind.insert(child, StageChildKind::GroupList);
                        handled = true;
                    }
                }

                if !handled && (kind == StageChildKind::MwndList || kind == StageChildKind::Unknown) {
                    if dispatch_mwnd_list_op(ctx, st, stage, op as i32, script_args, ret_form) {
                        st.child_kind.insert(child, StageChildKind::MwndList);
                        handled = true;
                    }
                }

                if !handled {
                    // Keep VM moving with a conservative default.
                    ctx.unknown.record_element_chain(form_id, &chain, "STAGE");
                    if let Some(rf) = ret_form {
                        if rf != 0 {
                            ctx.stack.push(default_for_ret_form(rf));
                        } else {
                            ctx.stack.push(Value::Int(0));
                        }
                    } else {
                        ctx.stack.push(Value::Int(0));
                    }
                }
            });

            return Ok(true);
        }
        StageTarget::ChildItemOp { stage, child, idx, op, tail } => {
            let form_id = ctx.ids.form_global_stage;
            let stage_elm_object = ctx.ids.stage_elm_object;

            with_stage_state(ctx, form_id, |ctx, st| {
                // Known object list path (compat).
                let obj_kind = st.child_kind.get(&child).copied().unwrap_or(StageChildKind::Unknown);
                if child == stage_elm_object || obj_kind == StageChildKind::ObjectList {
                    if dispatch_object_op(
                        ctx,
                        st,
                        stage,
                        idx,
                        op as i32,
                        &tail,
                        script_args,
                        ret_form,
                        rhs,
                        al_id,
                    ) {
                        st.child_kind.insert(child, StageChildKind::ObjectList);
                        return;
                    }

                    ctx.unknown.record_element_chain(form_id, &chain, "STAGE");
                    if let Some(rf) = ret_form {
                        if rf != 0 {
                            ctx.stack.push(default_for_ret_form(rf));
                        } else {
                            ctx.stack.push(Value::Int(0));
                        }
                    } else {
                        ctx.stack.push(Value::Int(0));
                    }
                    return;
                }

                // Non-object children: try Group then MWND.
                let kind = st.child_kind.get(&child).copied().unwrap_or(StageChildKind::Unknown);
                let mut handled = false;

                // Bias toward Group to avoid menu wait-loops deadlocking.
                if kind == StageChildKind::GroupList || kind == StageChildKind::Unknown {
                    // Reject if any string argument is present: group ops are int-only.
                    if !script_args.iter().any(|v| matches!(v, Value::Str(_))) {
                        if dispatch_group_item_op(
                            ctx,
                            st,
                            stage,
                            idx.max(0) as usize,
                            op as i32,
                            script_args,
                            rhs,
                            al_id,
                            ret_form,
                        ) {
                            st.child_kind.insert(child, StageChildKind::GroupList);
                            handled = true;
                        }
                    }
                }

                if !handled && (kind == StageChildKind::MwndList || kind == StageChildKind::Unknown) {
                    if dispatch_mwnd_item_op(
                        ctx,
                        st,
                        stage,
                        idx.max(0) as usize,
                        op as i32,
                        script_args,
                        rhs,
                        al_id,
                        ret_form,
                    ) {
                        st.child_kind.insert(child, StageChildKind::MwndList);
                        handled = true;
                    }
                }

                if !handled {
                    ctx.unknown.record_element_chain(form_id, &chain, "STAGE");
                    if let Some(rf) = ret_form {
                        if rf != 0 {
                            ctx.stack.push(default_for_ret_form(rf));
                        } else {
                            ctx.stack.push(Value::Int(0));
                        }
                    } else {
                        ctx.stack.push(Value::Int(0));
                    }
                }
            });

            Ok(true)
        }
    }
}
