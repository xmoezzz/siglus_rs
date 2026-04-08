use anyhow::Result;

use crate::runtime::globals::{EditBoxListState, EditBoxOpKind};
use crate::runtime::{CommandContext, Value};

fn default_for_ret_form(ret_form: i32) -> Value {
    // Runtime heuristic used by property access: ret_form == 2 is string.
    if ret_form == 2 {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn editbox_cnt(ctx: &CommandContext) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .map(|cfg| cfg.indexed_count("EDITBOX"))
        .unwrap_or(0)
}

fn parse_element_chain(args: &[Value]) -> Option<(usize, Vec<i32>)> {
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(chain) = v {
            return Some((i, chain.clone()));
        }
    }
    None
}

fn is_array_code(elm_array: i32, code: i32) -> bool {
	// If `elm_array` is not known yet (default -1), accept any non-zero marker.
	if elm_array < 0 {
		return code != 0;
	}
	code == elm_array
}

fn is_editbox_like_chain(ctx: &CommandContext, form_id: u32, chain: &[i32]) -> bool {
    if chain.is_empty() || chain[0] as u32 != form_id {
        return false;
    }
    // EDITBOX list ops are either:
    // - [FORM, GET_SIZE]
    // - [FORM, ELM_ARRAY, idx, ...]
    if chain.len() == 2 {
		return !is_array_code(ctx.ids.elm_array, chain[1]);
    }
	chain.len() >= 3 && is_array_code(ctx.ids.elm_array, chain[1])
}

fn classify_op(list: &EditBoxListState, params: &[Value], ret_form: i32) -> EditBoxOpKind {
    let is_str_ret = ret_form == 2;
    if is_str_ret {
        return EditBoxOpKind::GetText;
    }

    let has_string = params.iter().any(|v| v.as_str().is_some());
    let int_params = params.iter().filter(|v| v.as_i64().is_some()).count();

    if has_string {
        if int_params >= 1 {
            return EditBoxOpKind::Create;
        }
        return EditBoxOpKind::SetText;
    }

    // Zero-arg: either focus/destroy/clear (void) or check_decided/check_canceled (int).
    if params.is_empty() {
        if ret_form != 0 {
            if !list.has_kind(EditBoxOpKind::CheckDecided) {
                return EditBoxOpKind::CheckDecided;
            }
            if !list.has_kind(EditBoxOpKind::CheckCanceled) {
                return EditBoxOpKind::CheckCanceled;
            }
            return EditBoxOpKind::CheckDecided;
        }

        // ret_form == 0
        if !list.has_kind(EditBoxOpKind::SetFocus) {
            return EditBoxOpKind::SetFocus;
        }
        if !list.has_kind(EditBoxOpKind::ClearInput) {
            return EditBoxOpKind::ClearInput;
        }
        if !list.has_kind(EditBoxOpKind::Destroy) {
            return EditBoxOpKind::Destroy;
        }
        return EditBoxOpKind::ClearInput;
    }

    // Single int param is commonly used for focus on/off.
    if params.len() == 1 && params[0].as_i64().is_some() && ret_form == 0 {
        return EditBoxOpKind::SetFocus;
    }

    // Heuristic: many int params => create.
    if int_params >= 4 && ret_form == 0 {
        return EditBoxOpKind::Create;
    }

    EditBoxOpKind::Unknown
}

fn apply_op(
    list: &mut EditBoxListState,
    form_id: u32,
    idx: usize,
    kind: EditBoxOpKind,
    params: &[Value],
    ret_form: i32,
) -> (Option<Value>, Option<Option<(u32, usize)>>) {
    if idx >= list.boxes.len() {
        let r = if ret_form != 0 {
            Some(default_for_ret_form(ret_form))
        } else {
            None
        };
        return (r, None);
    }

    let eb = &mut list.boxes[idx];
    match kind {
        EditBoxOpKind::Create => {
            eb.alive = true;
            eb.decided = false;
            eb.canceled = false;
            if params.len() >= 5 {
                if let Some(v) = params.get(4).and_then(|v| v.as_i64()) {
                    eb.moji_size = v as i32;
                }
            }
            if let Some(s) = params.iter().find_map(|v| v.as_str()) {
                eb.text = s.to_string();
            }
        }
        EditBoxOpKind::Destroy => {
            eb.alive = false;
            eb.text.clear();
            eb.decided = false;
            eb.canceled = false;
            return (None, Some(None));
        }
        EditBoxOpKind::SetText => {
            if let Some(s) = params.iter().find_map(|v| v.as_str()) {
                eb.text = s.to_string();
            } else {
                eb.text.clear();
            }
        }
        EditBoxOpKind::GetText => {
            return (Some(Value::Str(eb.text.clone())), None);
        }
        EditBoxOpKind::SetFocus => {
            eb.alive = true;
            eb.decided = false;
            eb.canceled = false;
            return (None, Some(Some((form_id, idx))));
        }
        EditBoxOpKind::ClearInput => {
            eb.text.clear();
            eb.decided = false;
            eb.canceled = false;
        }
        EditBoxOpKind::CheckDecided => {
            let v = if eb.decided { 1 } else { 0 };
            if eb.decided {
                eb.decided = false;
            }
            return (Some(Value::Int(v)), None);
        }
        EditBoxOpKind::CheckCanceled => {
            let v = if eb.canceled { 1 } else { 0 };
            if eb.canceled {
                eb.canceled = false;
            }
            return (Some(Value::Int(v)), None);
        }
        EditBoxOpKind::Unknown => {
            if ret_form != 0 {
                return (Some(default_for_ret_form(ret_form)), None);
            }
        }
    }

    (None, None)
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = parse_element_chain(args) else {
        return Ok(false);
    };
    if !is_editbox_like_chain(ctx, form_id, &chain) {
        return Ok(false);
    }

    // Handle assignment shape: [op_id, al_id, rhs, Element(chain)]
    if chain_pos == 3 {
        let al_id = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let rhs = args.get(2);
		if chain.len() >= 3 && is_array_code(ctx.ids.elm_array, chain[1]) {
            let idx = chain.get(2).copied().unwrap_or(0);
            let idx_usize = if idx < 0 { 0 } else { idx as usize };
            if al_id == 1 {
                if let Some(s) = rhs.and_then(|v| v.as_str()) {
                    let cnt = editbox_cnt(ctx);
                    let list = ctx
                        .globals
                        .editbox_lists
                        .entry(form_id)
                        .or_insert_with(|| EditBoxListState::new(cnt));
                    list.ensure_size(cnt);
                    let _ = apply_op(
                        list,
                        form_id,
                        idx_usize,
                        EditBoxOpKind::SetText,
                        &[Value::Str(s.to_string())],
                        0,
                    );
                }
            }
        }
        return Ok(true);
    }

    // Method-call shape:
    // [op_id, param0, ..., Element(chain), al_id, ret_form]
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let al_id = args
        .get(chain_pos + 1)
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let ret_form = args
        .get(chain_pos + 2)
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let _ = al_id;
	let elm_array = ctx.ids.elm_array;

    // Pre-compute index for focus side effects (used after dropping the `editbox_lists` borrow).
    let idx_for_focus = chain.get(2).copied().unwrap_or(0);
    let idx_usize_for_focus = if idx_for_focus < 0 {
        0
    } else {
        idx_for_focus as usize
    };

    // Decide whether we should confirm this form id as EDITBOXLIST before borrowing `editbox_lists`.
	let should_confirm = ctx.globals.guessed_editbox_form_id.is_none()
        && chain.len() >= 4
		&& is_array_code(elm_array, chain[1])
        && {
            let has_string = params.iter().any(|v| v.as_str().is_some());
            let int_params = params.iter().filter(|v| v.as_i64().is_some()).count();
            has_string || int_params >= 4
        };
    if should_confirm {
        ctx.globals.guessed_editbox_form_id = Some(form_id);
    }

    let cnt = editbox_cnt(ctx);
    let (handled, ret, focus_req): (bool, Option<Value>, Option<Option<(u32, usize)>>) = 'blk: {
        let list = ctx
            .globals
            .editbox_lists
            .entry(form_id)
            .or_insert_with(|| EditBoxListState::new(cnt));
        list.ensure_size(cnt);
        if should_confirm {
            list.confirmed = true;
        }

        // EDITBOX.GET_SIZE
        if chain.len() == 2 && !is_array_code(elm_array, chain[1]) {
            if ret_form != 0 {
                let r = Some(Value::Int(list.boxes.len() as i64));
                break 'blk (true, r, None);
            }
            // EDITBOXLIST_CLEAR_INPUT
            for eb in list.boxes.iter_mut() {
                eb.text.clear();
                eb.decided = false;
                eb.canceled = false;
            }
            break 'blk (true, None, Some(None));
        }

        // EDITBOX[idx]
		if chain.len() < 3 || !is_array_code(elm_array, chain[1]) {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r, None);
        }

        let idx_usize = idx_usize_for_focus;
        if idx_usize >= list.boxes.len() {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r, None);
        }

        if chain.len() == 3 {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r, None);
        }

        let op = chain[3];
        let kind = if let Some(k) = list.op_map.get(&op).copied() {
            k
        } else {
            let k = classify_op(list, params, ret_form);
            list.op_map.insert(op, k);
            k
        };
        let (r, fr) = apply_op(
            list,
            form_id,
            idx_usize,
            kind,
            params,
            ret_form,
        );
        break 'blk (true, r, fr);
    };

    if let Some(v) = ret {
        ctx.push(v);
    }
    if let Some(fr) = focus_req {
        match fr {
            Some(tgt) => {
                ctx.globals.focused_editbox = Some(tgt);
            }
            None => {
                if ctx.globals.focused_editbox == Some((form_id, idx_usize_for_focus)) {
                    ctx.globals.focused_editbox = None;
                }
            }
        }
    }
    Ok(handled)
}

/// Conservative dispatch for unknown global forms.
///
/// This is used when `form_global_editbox` is not mapped in `IdMap`.
pub fn maybe_dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    // If explicitly mapped, the main dispatcher will call `dispatch` directly.
    if ctx.ids.form_global_editbox != 0 {
        return Ok(false);
    }

    // Avoid false positives if the title does not define editboxes.
    if editbox_cnt(ctx) == 0 {
        return Ok(false);
    }

    // If we've already guessed, only accept that exact form id.
    if let Some(fid) = ctx.globals.guessed_editbox_form_id {
        if fid != form_id {
            return Ok(false);
        }
        return dispatch(ctx, form_id, args);
    }

    // Otherwise, require a strong-ish signature.
    let Some((chain_pos, chain)) = parse_element_chain(args) else {
        return Ok(false);
    };
    if !is_editbox_like_chain(ctx, form_id, &chain) {
        return Ok(false);
    }
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let has_string = params.iter().any(|v| v.as_str().is_some());
    let int_params = params.iter().filter(|v| v.as_i64().is_some()).count();
    if !has_string && int_params < 4 {
        return Ok(false);
    }

    dispatch(ctx, form_id, args)
}
