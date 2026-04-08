use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Datelike, Timelike, Local};
use std::fs::{self, OpenOptions};
use std::io::Write;

use crate::runtime::{CommandContext, Value};

use super::prop_access;

const GET_CALENDAR: i32 = 0;
const GET_UNIX_TIME: i32 = 1;
const CHECK_ACTIVE: i32 = 2;
const CHECK_DEBUG_FLAG: i32 = 3;
const SHELL_OPEN_FILE: i32 = 4;
const SHELL_OPEN_WEB: i32 = 5;
const CHECK_FILE_EXIST: i32 = 6;
const CHECK_FILE_EXIST_SAVE_DIR: i32 = 7;
const CHECK_DUMMY_FILE_ONCE: i32 = 8;
const CLEAR_DUMMY_FILE: i32 = 9;
const MESSAGEBOX_OK: i32 = 10;
const MESSAGEBOX_OKCANCEL: i32 = 11;
const MESSAGEBOX_YESNO: i32 = 12;
const MESSAGEBOX_YESNOCANCEL: i32 = 13;
const DEBUG_MESSAGEBOX_OK: i32 = 14;
const DEBUG_MESSAGEBOX_OKCANCEL: i32 = 15;
const DEBUG_MESSAGEBOX_YESNO: i32 = 16;
const DEBUG_MESSAGEBOX_YESNOCANCEL: i32 = 17;
const DEBUG_WRITE_LOG: i32 = 18;
const GET_SPEC_INFO_FOR_CHIHAYA_BENCH: i32 = 19;
const OPEN_DIALOG_FOR_CHIHAYA_BENCH: i32 = 20;
const GET_LANGUAGE: i32 = 21;

struct Call<'a> {
    op: i32,
    params: &'a [Value],
}

fn parse_call(form_id: u32, args: &[Value]) -> Option<Call<'_>> {
    if let Some((chain_pos, chain)) = prop_access::parse_element_chain(form_id, args) {
        if chain.len() >= 2 {
            let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
            return Some(Call { op: chain[1], params });
        }
    }
    let op = args.get(0).and_then(|v| v.as_i64())? as i32;
    Some(Call { op, params: &args[1..] })
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
    let Some(call) = parse_call(form_id, args) else {
        return Ok(false);
    };

    match call.op {
        GET_CALENDAR => {
            // Original engine writes local-time fields to the supplied element destinations.
            // Runtime: support the common case where the parameters are element chains.
            let tm = local_time_fields();
            let vals = [tm.0, tm.1, tm.2, tm.3, tm.4, tm.5, tm.6, tm.7];
            for (idx, value) in vals.iter().enumerate() {
                if let Some(Value::Element(chain)) = call.params.get(idx) {
                    prop_access::assign_to_chain(ctx, chain, Value::Int(*value));
                }
            }
            ctx.push(Value::Int(0));
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
            ctx.push(Value::Int(if ctx.globals.system.active_flag { 1 } else { 0 }));
            return Ok(true);
        }
        CHECK_DEBUG_FLAG => {
            ctx.push(Value::Int(if ctx.globals.system.debug_flag { 1 } else { 0 }));
            return Ok(true);
        }
        SHELL_OPEN_FILE => {
            let path = join_game_path(&ctx.project_dir, p_str(call.params, 0));
            ctx.globals.system.debug_logs.push(format!("shell_open_file:{}", path.display()));
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        SHELL_OPEN_WEB => {
            ctx.globals.system.debug_logs.push(format!("shell_open_web:{}", p_str(call.params, 0)));
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        CHECK_FILE_EXIST => {
            let path = join_game_path(&ctx.project_dir, p_str(call.params, 0));
            ctx.push(Value::Int(if path.exists() { 1 } else { 0 }));
            return Ok(true);
        }
        CHECK_FILE_EXIST_SAVE_DIR => {
            let save_dir = ctx.project_dir.join("save");
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
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        CLEAR_DUMMY_FILE => {
            ctx.globals.system.dummy_checks.clear();
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        MESSAGEBOX_OK | MESSAGEBOX_OKCANCEL => {
            // Without a native dialog, default to "OK" (0).
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        MESSAGEBOX_YESNO | MESSAGEBOX_YESNOCANCEL => {
            // Default to "YES" (0).
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        DEBUG_MESSAGEBOX_OK | DEBUG_MESSAGEBOX_OKCANCEL => {
            // Only meaningful in debug builds; default to OK.
            if ctx.globals.system.debug_flag {
                ctx.globals.system.debug_logs.push("debug_messagebox_ok".to_string());
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        DEBUG_MESSAGEBOX_YESNO | DEBUG_MESSAGEBOX_YESNOCANCEL => {
            if ctx.globals.system.debug_flag {
                ctx.globals.system.debug_logs.push("debug_messagebox_yesno".to_string());
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        DEBUG_WRITE_LOG => {
            if ctx.globals.system.debug_flag {
                let s = match call.params.get(0) {
                    Some(Value::Int(v)) => v.to_string(),
                    Some(Value::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                write_debug_log(&ctx.project_dir, &s);
                ctx.globals.system.debug_logs.push(s);
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        GET_SPEC_INFO_FOR_CHIHAYA_BENCH => {
            ctx.push(Value::Str("siglus_scene_vm".to_string()));
            return Ok(true);
        }
        OPEN_DIALOG_FOR_CHIHAYA_BENCH => {
            ctx.globals.system.bench_dialogs.push(p_str(call.params, 0).to_string());
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        GET_LANGUAGE => {
            ctx.push(Value::Str(ctx.globals.system.language_code.clone()));
            return Ok(true);
        }
        _ => {}
    }

    prop_access::dispatch_stateful_form(ctx, form_id, args);
    Ok(true)
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

fn write_debug_log(project_dir: &Path, msg: &str) {
    if msg.is_empty() {
        return;
    }
    let dir = project_dir.join("__DEBUG_LOG");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("debug_log.txt");
    let now = Local::now();
    let stamp = now.format("[%Y-%m-%d %H:%M:%S]").to_string();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}\t{}", stamp, msg);
    }
}
