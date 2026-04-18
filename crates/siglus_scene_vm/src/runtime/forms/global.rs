use anyhow::Result;

use crate::runtime::{constants, forms, CommandContext, Value};

use crate::runtime::forms::{
    cgtable, counter, database, editbox, file, frame_action, frame_action_ch, g00buf, input,
    int_event, int_list, key, keylist, mask, math, mouse, script, stage, steam, str_list, syscom,
    system, timewait,
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

pub fn dispatch_global_form(
    ctx: &mut CommandContext,
    form_id: u32,
    args: &[Value],
) -> Result<bool> {
    let form_id = canonical_global_form_id(ctx, form_id);

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
