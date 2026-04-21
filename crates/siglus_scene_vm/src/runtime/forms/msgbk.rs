//! Global MSGBK form, aligned to `cmd_mwnd.cpp::tnm_command_proc_msgbk()`.

use anyhow::Result;

use crate::runtime::globals::MsgBackState;
use crate::runtime::{CommandContext, Value};

use super::prop_access;

struct Call<'a> {
    op: i32,
    params: &'a [Value],
}

fn msgbk_state_mut(ctx: &mut CommandContext, form_id: u32) -> &mut MsgBackState {
    ctx.globals.msgbk_forms.entry(form_id).or_default()
}

fn parse_call<'a>(ctx: &CommandContext, args: &'a [Value]) -> Option<(u32, Call<'a>)> {
    let form_id = ctx.ids.form_global_msgbk;
    if form_id == 0 {
        return None;
    }
    let (chain_pos, chain) = prop_access::parse_element_chain_ctx(ctx, form_id, args)?;
    if chain.len() < 2 {
        return None;
    }
    Some((
        form_id,
        Call {
            op: chain[1],
            params: prop_access::script_args(args, chain_pos),
        },
    ))
}

fn arg_i64(args: &[Value], idx: usize) -> Option<i64> {
    args.get(idx).and_then(Value::as_i64)
}

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    args.get(idx).and_then(Value::as_str)
}

fn named_i64(args: &[Value], id: i32) -> Option<i64> {
    args.iter()
        .find(|v| v.named_id() == Some(id))
        .and_then(Value::as_i64)
}

fn named_str<'a>(args: &'a [Value], id: i32) -> Option<&'a str> {
    args.iter()
        .find(|v| v.named_id() == Some(id))
        .and_then(Value::as_str)
}

fn resolve_debug_open_scene(ctx: &CommandContext, params: &[Value]) -> Result<(i64, i64)> {
    let scene_name = named_str(params, 0).unwrap_or("");
    let mut scene_no = if !scene_name.is_empty() {
        ctx.lookup_scene_no(scene_name)?
    } else {
        ctx.current_scene_no.unwrap_or(-1)
    };
    let mut line_no = named_i64(params, 1).unwrap_or(0);
    if line_no <= 0 {
        line_no = ctx.current_line_no;
    }
    if scene_no < 0 {
        scene_no = -1;
    }
    Ok((scene_no, line_no))
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((form_id, call)) = parse_call(ctx, args) else {
        return Ok(false);
    };

    match call.op {
        op if op == crate::runtime::constants::MSGBK_INSERT_IMG => {
            let file = arg_str(call.params, 0).unwrap_or("");
            let x = arg_i64(call.params, 1).unwrap_or(0) as i32;
            let y = arg_i64(call.params, 2).unwrap_or(0) as i32;
            if !file.is_empty() {
                msgbk_state_mut(ctx, form_id).add_pct(file, x, y);
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        op if op == crate::runtime::constants::MSGBK_INSERT_MSG => {
            let msg = arg_str(call.params, 0).unwrap_or("");
            let (scene_no, line_no) = resolve_debug_open_scene(ctx, call.params)?;
            if scene_no >= 0 {
                let st = msgbk_state_mut(ctx, form_id);
                st.next();
                st.add_msg(msg, msg, scene_no, line_no);
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        op if op == crate::runtime::constants::MSGBK_ADD_KOE => {
            let koe_no = arg_i64(call.params, 0).unwrap_or(0);
            let chara_no = arg_i64(call.params, 1).unwrap_or(-1);
            let (scene_no, line_no) = resolve_debug_open_scene(ctx, call.params)?;
            if scene_no >= 0 {
                msgbk_state_mut(ctx, form_id).add_koe(koe_no, chara_no, scene_no, line_no);
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        op if op == crate::runtime::constants::MSGBK_ADD_NAMAE => {
            let name = arg_str(call.params, 0).unwrap_or("");
            let (scene_no, line_no) = resolve_debug_open_scene(ctx, call.params)?;
            if scene_no >= 0 {
                msgbk_state_mut(ctx, form_id).add_name(name, name, scene_no, line_no);
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        op if op == crate::runtime::constants::MSGBK_ADD_MSG => {
            let msg = arg_str(call.params, 0).unwrap_or("");
            let (scene_no, line_no) = resolve_debug_open_scene(ctx, call.params)?;
            if scene_no >= 0 {
                msgbk_state_mut(ctx, form_id).add_msg(msg, msg, scene_no, line_no);
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        op if op == crate::runtime::constants::MSGBK_GO_NEXT_MSG => {
            msgbk_state_mut(ctx, form_id).next();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => Ok(false),
    }
}
