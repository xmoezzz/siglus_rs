use anyhow::Result;

use super::prop_access;
use crate::runtime::constants;
use crate::runtime::globals::EditBoxListState;
use crate::runtime::{CommandContext, Value};

fn default_for_ret_form(ret_form: i32) -> Value {
    if prop_access::ret_form_is_string(ret_form as i64) {
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

fn is_array_code(elm_array: i32, code: i32) -> bool {
    if elm_array < 0 {
        return code != 0;
    }
    code == elm_array
}

fn is_editbox_like_chain(ctx: &CommandContext, form_id: u32, chain: &[i32]) -> bool {
    if chain.is_empty() || chain[0] as u32 != form_id {
        return false;
    }
    if chain.len() == 2 {
        return !is_array_code(ctx.ids.elm_array, chain[1]);
    }
    chain.len() >= 3 && is_array_code(ctx.ids.elm_array, chain[1])
}

fn apply_exact_op(
    form_id: u32,
    list: &mut EditBoxListState,
    idx: usize,
    op: i32,
    params: &[Value],
    ret_form: i32,
    screen_w: i32,
    screen_h: i32,
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
    match op {
        x if x == constants::elm_value::EDITBOX_CREATE => {
            let x = params.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let w = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let h = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let moji_size = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            eb.create_like(x, y, w, h, moji_size, screen_w, screen_h);
            eb.update_rect(screen_w, screen_h);
            eb.frame(0);
            (None, Some(Some((form_id, idx))))
        }
        x if x == constants::elm_value::EDITBOX_DESTROY => {
            eb.destroy_like();
            (None, Some(None))
        }
        x if x == constants::elm_value::EDITBOX_SET_TEXT => {
            eb.set_text_like(
                params
                    .iter()
                    .find_map(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            );
            (None, None)
        }
        x if x == constants::elm_value::EDITBOX_GET_TEXT => {
            (Some(Value::Str(eb.text.clone())), None)
        }
        x if x == constants::elm_value::EDITBOX_SET_FOCUS => {
            if eb.created {
                (None, Some(Some((form_id, idx))))
            } else {
                (None, None)
            }
        }
        x if x == constants::elm_value::EDITBOX_CLEAR_INPUT => {
            eb.clear_input();
            (None, None)
        }
        x if x == constants::elm_value::EDITBOX_CHECK_DECIDED => {
            (Some(Value::Int(if eb.is_decided() { 1 } else { 0 })), None)
        }
        x if x == constants::elm_value::EDITBOX_CHECK_CANCELED => {
            (Some(Value::Int(if eb.is_canceled() { 1 } else { 0 })), None)
        }
        _ => {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            (r, None)
        }
    }
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) =
        prop_access::parse_element_chain_ctx(ctx, form_id, args).map(|(i, ch)| (i, ch.to_vec()))
    else {
        return Ok(false);
    };
    if !is_editbox_like_chain(ctx, form_id, &chain) {
        return Ok(false);
    }

    let (meta_al_id, _meta_ret_form, rhs_meta) =
        prop_access::infer_assign_and_ret_ctx(ctx, chain_pos, args);
    if chain.len() >= 4 && is_array_code(ctx.ids.elm_array, chain[1]) && meta_al_id == Some(1) {
        let rhs = rhs_meta.as_ref();
        let idx = chain.get(2).copied().unwrap_or(0).max(0) as usize;
        let op = chain[3];
        let cnt = editbox_cnt(ctx);
        let list = ctx
            .globals
            .editbox_lists
            .entry(form_id)
            .or_insert_with(|| EditBoxListState::new(cnt));
        list.ensure_size(cnt);
        let params = match rhs {
            Some(Value::Str(s)) => vec![Value::Str(s.clone())],
            Some(v) => vec![v.clone()],
            None => Vec::new(),
        };
        let (_ret, focus_req) = apply_exact_op(
            form_id,
            list,
            idx,
            op,
            &params,
            0,
            ctx.screen_w as i32,
            ctx.screen_h as i32,
        );
        if let Some(req) = focus_req {
            match req {
                Some(tgt) => ctx.globals.focused_editbox = Some(tgt),
                None => {
                    if ctx.globals.focused_editbox == Some((form_id, idx)) {
                        ctx.globals.focused_editbox = None;
                    }
                }
            }
        }
        return Ok(true);
    }

    let params = prop_access::script_args(args, chain_pos);
    let ret_form = crate::runtime::forms::prop_access::current_vm_meta(ctx)
        .1
        .unwrap_or(0) as i32;
    let elm_array = ctx.ids.elm_array;

    let idx_for_focus = chain.get(2).copied().unwrap_or(0).max(0) as usize;
    let cnt = editbox_cnt(ctx);
    let (handled, ret, focus_req): (bool, Option<Value>, Option<Option<(u32, usize)>>) = 'blk: {
        let list = ctx
            .globals
            .editbox_lists
            .entry(form_id)
            .or_insert_with(|| EditBoxListState::new(cnt));
        list.ensure_size(cnt);

        if chain.len() == 2 && !is_array_code(elm_array, chain[1]) {
            let op = chain[1];
            if op == constants::elm_value::EDITBOXLIST_CLEAR_INPUT && ret_form == 0 {
                for eb in list.boxes.iter_mut() {
                    eb.clear_input();
                }
                break 'blk (true, None, None);
            }
            if ret_form != 0 {
                break 'blk (true, Some(Value::Int(list.boxes.len() as i64)), None);
            }
            break 'blk (true, None, None);
        }

        if chain.len() < 4 || !is_array_code(elm_array, chain[1]) {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r, None);
        }

        let idx = chain.get(2).copied().unwrap_or(0).max(0) as usize;
        let op = chain[3];
        let (r, fr) = apply_exact_op(
            form_id,
            list,
            idx,
            op,
            params,
            ret_form,
            ctx.screen_w as i32,
            ctx.screen_h as i32,
        );
        break 'blk (true, r, fr);
    };

    if let Some(v) = ret {
        ctx.push(v);
    }
    if let Some(fr) = focus_req {
        match fr {
            Some(tgt) => ctx.globals.focused_editbox = Some(tgt),
            None => {
                if ctx.globals.focused_editbox == Some((form_id, idx_for_focus)) {
                    ctx.globals.focused_editbox = None;
                }
            }
        }
    }
    Ok(handled)
}
