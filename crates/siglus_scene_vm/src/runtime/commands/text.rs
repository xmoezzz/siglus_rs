use anyhow::Result;

use crate::runtime::{Command, CommandContext, Value};
use crate::runtime::forms::stage;

use super::util::strip_vm_meta;

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
            ctx.ui.show_message_bg(true);
            ctx.push(Value::Int(0));
        }
        "MWNDCLOSE" => {
            ctx.ui.show_message_bg(false);
            ctx.push(Value::Int(0));
        }
        "NAME" => {
            if let Some(Value::Str(s)) = args.first() {
                ctx.ui.show_message_bg(true);
                if !stage::cd_name_current_mwnd(ctx, s) {
                    ctx.ui.set_name(s.clone());
                }
            }
            ctx.push(Value::Int(0));
        }
        _ => {
            if let Some(Value::Str(s)) = args.first() {
                ctx.ui.show_message_bg(true);
                ctx.ui.set_message(s.clone());
                eprintln!("[VM-MSG] {}", s);
            }
            ctx.push(Value::Int(0));
        }
    }

    Ok(true)
}
