use anyhow::Result;
use chrono::{Datelike, Local, Timelike};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::{CommandContext, Value};

use super::prop_access;
use super::syscom;

const CHECK_ACTIVE: i32 = 0;
const SHELL_OPEN_FILE: i32 = 1;
const CHECK_DUMMY_FILE_ONCE: i32 = 2;
const OPEN_DIALOG_FOR_CHIHAYA_BENCH: i32 = 3;
const GET_SPEC_INFO_FOR_CHIHAYA_BENCH: i32 = 4;
const SHELL_OPEN_WEB: i32 = 5;
const CHECK_FILE_EXIST: i32 = 6;
const DEBUG_MESSAGEBOX_OK: i32 = 7;
const DEBUG_MESSAGEBOX_OKCANCEL: i32 = 8;
const DEBUG_MESSAGEBOX_YESNO: i32 = 9;
const DEBUG_MESSAGEBOX_YESNOCANCEL: i32 = 10;
const DEBUG_WRITE_LOG: i32 = 11;
const CHECK_FILE_EXIST_SAVE_DIR: i32 = 12;
const CHECK_DEBUG_FLAG: i32 = 13;
const GET_CALENDAR: i32 = 14;
const GET_UNIX_TIME: i32 = 15;
const GET_LANGUAGE: i32 = 16;
const MESSAGEBOX_OK: i32 = 17;
const MESSAGEBOX_OKCANCEL: i32 = 18;
const MESSAGEBOX_YESNO: i32 = 19;
const MESSAGEBOX_YESNOCANCEL: i32 = 20;
const CLEAR_DUMMY_FILE: i32 = 21;

struct Call<'a> {
    op: i32,
    params: &'a [Value],
}

fn parse_call<'a>(ctx: &CommandContext, form_id: u32, args: &'a [Value]) -> Option<Call<'a>> {
    let (chain_pos, chain) = prop_access::parse_element_chain_ctx(ctx, form_id, args)?;
    if chain.len() < 2 {
        return None;
    }
    let params = prop_access::script_args(args, chain_pos);
    Some(Call {
        op: chain[1],
        params,
    })
}

fn p_str(params: &[Value], idx: usize) -> &str {
    params.get(idx).and_then(|v| v.as_str()).unwrap_or("")
}

fn join_game_path(base: &Path, raw: &str) -> PathBuf {
    if raw.is_empty() {
        return base.to_path_buf();
    }
    let norm = raw.replace('\\', "/");
    let p = Path::new(&norm);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some(call) = parse_call(ctx, form_id, args) else {
        return Ok(false);
    };

    match call.op {
        GET_CALENDAR => {
            let tm = local_time_fields();
            let vals = [tm.0, tm.1, tm.2, tm.3, tm.4, tm.5, tm.6, tm.7];
            for (idx, value) in vals.iter().enumerate() {
                if let Some(Value::Element(chain)) = call.params.get(idx) {
                    prop_access::assign_to_chain(ctx, chain, Value::Int(*value));
                }
            }
            return Ok(true);
        }
        GET_UNIX_TIME => {
            let t = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            ctx.push(Value::Int(t));
            return Ok(true);
        }
        CHECK_ACTIVE => {
            ctx.push(Value::Int(if ctx.globals.system.active_flag {
                1
            } else {
                0
            }));
            return Ok(true);
        }
        CHECK_DEBUG_FLAG => {
            ctx.push(Value::Int(if ctx.globals.system.debug_flag {
                1
            } else {
                0
            }));
            return Ok(true);
        }
        SHELL_OPEN_FILE => {
            let path = join_game_path(&ctx.project_dir, p_str(call.params, 0));
            if path.exists() {
                let _ = ctx.net.open_file(&path);
            }
            ctx.globals
                .system
                .debug_logs
                .push(format!("shell_open_file:{}", path.display()));
            return Ok(true);
        }
        SHELL_OPEN_WEB => {
            let url = p_str(call.params, 0);
            let _ = ctx.net.open_url(url);
            ctx.globals
                .system
                .debug_logs
                .push(format!("shell_open_web:{url}"));
            return Ok(true);
        }
        CHECK_FILE_EXIST => {
            let path = join_game_path(&ctx.project_dir, p_str(call.params, 0));
            ctx.push(Value::Int(if path.exists() { 1 } else { 0 }));
            return Ok(true);
        }
        CHECK_FILE_EXIST_SAVE_DIR => {
            let save_dir = syscom::save_dir(&ctx.project_dir);
            let path = join_game_path(&save_dir, p_str(call.params, 0));
            ctx.push(Value::Int(if path.exists() { 1 } else { 0 }));
            return Ok(true);
        }
        CHECK_DUMMY_FILE_ONCE => {
            let name = p_str(call.params, 0);
            let key = call.params.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
            let code = p_str(call.params, 2);
            let sig = format!("{name}:{key}:{code}");
            ctx.globals.system.dummy_checks.insert(sig);
            return Ok(true);
        }
        CLEAR_DUMMY_FILE => {
            ctx.globals.system.dummy_checks.clear();
            return Ok(true);
        }
        MESSAGEBOX_OK | MESSAGEBOX_OKCANCEL | MESSAGEBOX_YESNO | MESSAGEBOX_YESNOCANCEL => {
            let text = messagebox_text(ctx, call.params);
            if let Some(ret) = handle_messagebox(ctx, call.op, false, text) {
                ctx.push(Value::Int(ret));
            }
            return Ok(true);
        }
        DEBUG_MESSAGEBOX_OK
        | DEBUG_MESSAGEBOX_OKCANCEL
        | DEBUG_MESSAGEBOX_YESNO
        | DEBUG_MESSAGEBOX_YESNOCANCEL => {
            let text = messagebox_text(ctx, call.params);
            if ctx.globals.system.debug_flag {
                if let Some(ret) = handle_messagebox(ctx, call.op, true, text) {
                    ctx.push(Value::Int(ret));
                }
            } else {
                ctx.push(Value::Int(0));
            }
            return Ok(true);
        }
        DEBUG_WRITE_LOG => {
            if ctx.globals.system.debug_flag {
                let s = match call.params.get(0) {
                    Some(Value::Int(v)) => v.to_string(),
                    Some(Value::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                write_debug_log(
                    &ctx.project_dir,
                    &s,
                    ctx.current_scene_name.as_deref(),
                    ctx.current_line_no,
                );
                ctx.globals.system.debug_logs.push(s);
            }
            return Ok(true);
        }
        GET_SPEC_INFO_FOR_CHIHAYA_BENCH => {
            ctx.push(Value::Str(ctx.globals.system.spec_info.clone()));
            return Ok(true);
        }
        OPEN_DIALOG_FOR_CHIHAYA_BENCH => {
            ctx.globals
                .system
                .bench_dialogs
                .push(p_str(call.params, 0).to_string());
            return Ok(true);
        }
        GET_LANGUAGE => {
            ctx.push(Value::Str(ctx.globals.system.language_code.clone()));
            return Ok(true);
        }
        _ => {}
    }

    Ok(false)
}

fn messagebox_text(ctx: &CommandContext, params: &[Value]) -> String {
    match params.first() {
        Some(Value::Int(v)) => v.to_string(),
        Some(Value::Str(s)) => s.clone(),
        Some(v) => v.as_str().unwrap_or("").to_string(),
        None => {
            if let Some(name) = ctx.current_scene_name.as_deref() {
                format!("{name}:{}", ctx.current_line_no)
            } else {
                String::new()
            }
        }
    }
}

fn handle_messagebox(
    ctx: &mut CommandContext,
    kind: i32,
    debug_only: bool,
    text: String,
) -> Option<i64> {
    ctx.globals
        .system
        .messagebox_history
        .push(crate::runtime::globals::SystemMessageBoxRecord {
            kind,
            text: text.clone(),
            debug_only,
        });

    let buttons = messagebox_buttons(kind);
    let max_value = buttons.iter().map(|b| b.value).max().unwrap_or(0);
    if !ctx.globals.system.messagebox_response_queue.is_empty() {
        let v = ctx.globals.system.messagebox_response_queue.remove(0);
        return Some(v.clamp(0, max_value));
    }

    ctx.request_system_messagebox(kind, debug_only, text, buttons, true);
    None
}

fn messagebox_buttons(kind: i32) -> Vec<crate::runtime::globals::SystemMessageBoxButton> {
    let raw: &[(&str, i64)] = match kind {
        MESSAGEBOX_OK | DEBUG_MESSAGEBOX_OK => &[("OK", 0)],
        MESSAGEBOX_OKCANCEL | DEBUG_MESSAGEBOX_OKCANCEL => &[("OK", 0), ("CANCEL", 1)],
        MESSAGEBOX_YESNO | DEBUG_MESSAGEBOX_YESNO => &[("YES", 0), ("NO", 1)],
        MESSAGEBOX_YESNOCANCEL | DEBUG_MESSAGEBOX_YESNOCANCEL => {
            &[("YES", 0), ("NO", 1), ("CANCEL", 2)]
        }
        _ => &[("OK", 0)],
    };
    raw.iter()
        .map(
            |(label, value)| crate::runtime::globals::SystemMessageBoxButton {
                label: (*label).to_string(),
                value: *value,
            },
        )
        .collect()
}

fn local_time_fields() -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let now = Local::now();
    let weekday = now.weekday().num_days_from_sunday() as i64;
    (
        now.year() as i64,
        now.month() as i64,
        now.day() as i64,
        weekday,
        now.hour() as i64,
        now.minute() as i64,
        now.second() as i64,
        now.nanosecond() as i64 / 1_000_000,
    )
}

fn write_debug_log(project_dir: &Path, msg: &str, scene_name: Option<&str>, line_no: i64) {
    if msg.is_empty() {
        return;
    }
    let dir = project_dir.join("__DEBUG_LOG");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("debug_log.txt");
    let now = Local::now();
    let stamp = now.format("[%Y-%m-%d %H:%M:%S]").to_string();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        if let Some(scene) = scene_name {
            let _ = writeln!(f, "{}\t({}.ss line={})\t{}", stamp, scene, line_no, msg);
        } else {
            let _ = writeln!(f, "{}\t{}", stamp, msg);
        }
    }
}
