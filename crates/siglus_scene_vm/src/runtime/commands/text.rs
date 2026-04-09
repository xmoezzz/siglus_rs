use anyhow::Result;

use crate::runtime::{Command, CommandContext, Value};

use super::util::strip_vm_meta;

fn ensure_default_msg_bg(ctx: &mut CommandContext) {
    if ctx.ui.msg_bg_image.is_some() {
        return;
    }
    // Simple placeholder (1x1) scaled by the UI layout logic.
    let img = ctx.images.solid_rgba((0, 0, 0, 160));
    ctx.ui.set_message_bg(img);
}

pub fn handle(ctx: &mut CommandContext, cmd: &Command) -> Result<bool> {
    let name = cmd.name.to_ascii_uppercase();

    if !matches!(
        name.as_str(),
        "MSG"
            | "MES"
            | "MESSAGE"
            | "TEXT"
            | "NAME"
            | "MWND"
            | "MWNDOPEN"
            | "MWNDCLOSE"
            | "MSGWAIT"
            | "MSG_WAIT"
            | "TEXTWAIT"
            | "TEXT_WAIT"
            | "WAITMSG"
            | "WAIT_TEXT"
            | "CLRMSG"
            | "CLEARMSG"
            | "CLR_TEXT"
    ) {
        return Ok(false);
    }

    let args = strip_vm_meta(&cmd.args);

    match name.as_str() {
        "MSGWAIT" | "MSG_WAIT" | "TEXTWAIT" | "TEXT_WAIT" | "WAITMSG" | "WAIT_TEXT" => {
            ctx.wait.wait_key();
            ctx.ui.begin_wait_message();
            ctx.push(Value::Int(0));
        }
        "CLRMSG" | "CLEARMSG" | "CLR_TEXT" => {
            ctx.ui.clear_message();
            ctx.ui.clear_name();
            ctx.ui.show_message_bg(false);
            ctx.push(Value::Int(0));
        }
        "MWND" | "MWNDOPEN" => {
            ensure_default_msg_bg(ctx);
            ctx.ui.show_message_bg(true);
            ctx.push(Value::Int(0));
        }
        "MWNDCLOSE" => {
            ctx.ui.show_message_bg(false);
            ctx.push(Value::Int(0));
        }
        "NAME" => {
            if let Some(Value::Str(s)) = args.get(0) {
                ensure_default_msg_bg(ctx);
                ctx.ui.show_message_bg(true);
                ctx.ui.set_name(s.clone());
            }
            ctx.push(Value::Int(0));
        }
        _ => {
            // MSG / MES / MESSAGE / TEXT
            if let Some(Value::Str(s)) = args.get(0) {
                ensure_default_msg_bg(ctx);
                ctx.ui.show_message_bg(true);
                ctx.ui.set_message(s.clone());
                eprintln!("[VM-MSG] {}", s);
            }
            ctx.push(Value::Int(0));
        }
    }

    Ok(true)
}
