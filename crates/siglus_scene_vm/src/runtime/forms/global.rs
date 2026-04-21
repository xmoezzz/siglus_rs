use anyhow::Result;

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

fn parse_selbtn_choices(args: &[Value]) -> (i64, Vec<crate::runtime::globals::BtnSelectChoiceState>) {
    let mut template_no = 0i64;
    let mut start = 0usize;
    if args.first().and_then(Value::as_i64).is_some() {
        template_no = args.first().and_then(Value::as_i64).unwrap_or(0);
        start = 1;
    }

    let mut out = Vec::new();
    let mut last: Option<usize> = None;
    let mut arg_no = 0i32;
    for v in args.iter().skip(start).map(Value::unwrap_named) {
        if let Some(s) = v.as_str() {
            out.push(crate::runtime::globals::BtnSelectChoiceState {
                text: s.to_string(),
                item_type: 0,
                color: -1,
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
        ctx.globals.selbtn.cursor = 0;
        ctx.globals.selbtn.cancel_enable = op == constants::elm_value::GLOBAL_SELBTN_CANCEL
            || op == constants::elm_value::GLOBAL_SELBTN_CANCEL_READY;
        ctx.globals.selbtn.capture_flag = if ready { false } else { capture_flag };
        ctx.globals.selbtn.sel_start_call_scn = if ready { String::new() } else { sel_start_call_scn };
        ctx.globals.selbtn.sel_start_call_z_no = if ready { 0 } else { sel_start_call_z_no };
        ctx.globals.selbtn.result = 0;
    }

    if start_now {
        ctx.globals.selbtn.started = true;
        ctx.globals.selbtn.sync_type = named_i64(args, 4).unwrap_or(0);
        ctx.globals.selbtn.read_flag_scene_no = ctx.current_scene_no.unwrap_or(-1);
        ctx.globals.selbtn.read_flag_flag_no = 0;
        ctx.wait.wait_key();
    } else {
        ctx.push(Value::Int(0));
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
    props.insert(constants::elm_value::GLOBAL_KOE_CHECK_GET_CHARA_NO, chara_no);
    props.insert(constants::elm_value::GLOBAL_KOE_CHECK_IS_EX_KOE, if is_ex { 1 } else { 0 });
}

fn remembered_global_koe(ctx: &CommandContext, op: i32) -> i64 {
    let key = global_koe_state_key(ctx);
    ctx.globals
        .int_props
        .get(&key)
        .and_then(|m| m.get(&op).copied())
        .unwrap_or(0)
}

fn dispatch_global_koe_command(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let op = form_id as i32;
    let ret_form: Option<i64> = crate::runtime::forms::prop_access::current_vm_meta(ctx).1;
    match op {
        constants::elm_value::GLOBAL_KOE | constants::elm_value::GLOBAL_EXKOE => {
            let koe_no = args.get(0).and_then(Value::as_i64).unwrap_or(0);
            let chara_no = args.get(1).and_then(Value::as_i64).unwrap_or(0);
            let is_ex = op == constants::elm_value::GLOBAL_EXKOE;
            remember_global_koe(ctx, koe_no, chara_no, is_ex);
            let _ = {
                let (se, audio) = (&mut ctx.se, &mut ctx.audio);
                se.play_koe_no(audio, koe_no)
            };
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_PLAY_WAIT
        | constants::elm_value::GLOBAL_KOE_PLAY_WAIT_KEY
        | constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT
        | constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY => {
            let koe_no = args.get(0).and_then(Value::as_i64).unwrap_or(0);
            let chara_no = args.get(1).and_then(Value::as_i64).unwrap_or(0);
            let is_ex = op == constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT
                || op == constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY;
            remember_global_koe(ctx, koe_no, chara_no, is_ex);
            let _ = {
                let (se, audio) = (&mut ctx.se, &mut ctx.audio);
                se.play_koe_no(audio, koe_no)
            };
            let key_skip = op == constants::elm_value::GLOBAL_KOE_PLAY_WAIT_KEY
                || op == constants::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY;
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::SeAny, key_skip);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_STOP => {
            let fade = args.get(0).and_then(Value::as_i64);
            let _ = ctx.se.stop(fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_WAIT | constants::elm_value::GLOBAL_KOE_WAIT_KEY => {
            let key_skip = op == constants::elm_value::GLOBAL_KOE_WAIT_KEY;
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::SeAny, key_skip);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_CHECK => {
            let playing = ctx.se.is_playing_any();
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
            let vol = args.get(0).and_then(Value::as_i64).unwrap_or(255).clamp(0, 255) as u8;
            let fade = args.get(1).and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.se.set_volume_raw_fade(&mut ctx.audio, vol, fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_SET_VOLUME_MAX => {
            let fade = args.get(0).and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.se.set_volume_raw_fade(&mut ctx.audio, 255, fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_SET_VOLUME_MIN => {
            let fade = args.get(0).and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.se.set_volume_raw_fade(&mut ctx.audio, 0, fade);
            Ok(true)
        }
        constants::elm_value::GLOBAL_KOE_GET_VOLUME => {
            ctx.push(Value::Int(ctx.se.volume_raw() as i64));
            Ok(true)
        }
        _ => Ok(false),
    }
}


fn dispatch_capture_command(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    match form_id as i32 {
        constants::elm_value::GLOBAL_CAPTURE => {
            let img = ctx.capture_frame_rgba();
            ctx.globals.capture_image = Some(img);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FREE => {
            ctx.globals.capture_image = None;
            ctx.push(Value::Int(0));
            Ok(true)
        }
        constants::elm_value::GLOBAL_CAPTURE_FROM_FILE => {
            let Some(file) = args.get(0).and_then(|v| v.as_str()) else {
                panic!("GLOBAL.CAPTURE_FROM_FILE requires file name");
            };
            let Some(path) = stage::resolve_capture_file_path(&ctx.project_dir, &ctx.globals.append_dir, file) else {
                panic!("GLOBAL.CAPTURE_FROM_FILE cannot resolve file: {file}");
            };
            let img_id = ctx
                .images
                .load_file(&path, 0)
                .unwrap_or_else(|e| panic!("GLOBAL.CAPTURE_FROM_FILE failed to load {}: {e}", path.display()));
            let img = ctx
                .images
                .get(img_id)
                .map(|img| img.as_ref().clone())
                .unwrap_or_else(|| panic!("GLOBAL.CAPTURE_FROM_FILE image disappeared: {}", path.display()));
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
            let img = ctx.capture_frame_rgba();
            ctx.globals.capture_image = Some(img);
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

pub fn dispatch_global_form(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    let form_id = canonical_global_form_id(ctx, form_id);

    if dispatch_capture_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if dispatch_selbtn_command(ctx, form_id, args)? {
        return Ok(true);
    }
    if dispatch_global_koe_command(ctx, form_id, args)? {
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
    if form_id == 6 || form_id == 96 {
        ctx.wait.wait_next_frame(ctx.globals.render_frame);
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
