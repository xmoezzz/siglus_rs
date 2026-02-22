use anyhow::Result;

use crate::runtime::{forms, CommandContext, Value};

use crate::runtime::forms::{
    counter, input, int_list, key, keylist, mouse, stage, str_list, stub, timewait,
    math, cgtable, database, g00buf, mask, editbox, file, steam,
    syscom, script, system, frame_action, frame_action_ch,
};
use crate::runtime::forms::codes;

pub fn dispatch_global_form(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    // Prefer externally configured numeric IDs.
    if form_id == ctx.ids.form_global_stage {
        return stage::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_bgm {
        return forms::bgm::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_bgm_table {
        return forms::bgm_table::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_mov {
        return forms::mov::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_pcm {
        return forms::pcm::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_pcmch {
        return forms::pcmch::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_se {
        return forms::se::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_pcm_event {
        return forms::pcmevent::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_excall {
        return forms::excall::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_koe_st {
        return forms::koe_st::dispatch(ctx, args);
    }

    if form_id == ctx.ids.form_global_input {
        return input::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_mouse {
        return mouse::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_keylist {
        return keylist::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_key {
        return key::dispatch(ctx, args);
    }

    if form_id == ctx.ids.form_global_screen {
        return forms::screen::dispatch(ctx, args);
    }
    if form_id == ctx.ids.form_global_msgbk {
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
    if ctx.ids.form_global_frame_action != 0 && form_id == ctx.ids.form_global_frame_action {
        return frame_action::dispatch(ctx, form_id, args);
    }
    if ctx.ids.form_global_frame_action_ch != 0 && form_id == ctx.ids.form_global_frame_action_ch {
        return frame_action_ch::dispatch(ctx, form_id, args);
    }

    // Fallback to historical hard-coded constants.
    match form_id {
        codes::FORM_GLOBAL_BGM => forms::bgm::dispatch(ctx, args),
        codes::FORM_GLOBAL_BGM_TABLE => forms::bgm_table::dispatch(ctx, args),
        codes::FORM_GLOBAL_MOV => forms::mov::dispatch(ctx, args),
        codes::FORM_GLOBAL_PCM => forms::pcm::dispatch(ctx, args),
        codes::FORM_GLOBAL_PCMCH => forms::pcmch::dispatch(ctx, args),
        codes::FORM_GLOBAL_SE => forms::se::dispatch(ctx, args),
        codes::FORM_GLOBAL_PCMEVENT => forms::pcmevent::dispatch(ctx, args),
        codes::FORM_GLOBAL_EXCALL => forms::excall::dispatch(ctx, args),
        codes::FORM_GLOBAL_KOE_ST => forms::koe_st::dispatch(ctx, args),
        codes::FORM_GLOBAL_SCREEN => forms::screen::dispatch(ctx, args),
        codes::FORM_GLOBAL_MSGBK => forms::msgbk::dispatch(ctx, args),
        codes::FORM_GLOBAL_KEY => key::dispatch(ctx, args),
        _ => {
            // TIMEWAIT/TIMEWAIT_KEY are statement-like forms that block execution.
            if form_id == codes::FORM_GLOBAL_TIMEWAIT {
                return timewait::dispatch(ctx, false, args);
            }
            if form_id == codes::FORM_GLOBAL_TIMEWAIT_KEY {
                return timewait::dispatch(ctx, true, args);
            }

            // Mask list (if mapped via IdMap).
            if ctx.ids.form_global_mask != 0 && form_id == ctx.ids.form_global_mask {
                return mask::dispatch(ctx, form_id, args);
            }

            // EditBox list (if mapped via IdMap).
            if ctx.ids.form_global_editbox != 0 && form_id == ctx.ids.form_global_editbox {
                return editbox::dispatch(ctx, form_id, args);
            }

            // Auto-detect mask list if no mapping is provided.
            if mask::maybe_dispatch(ctx, form_id, args)? {
                return Ok(true);
            }

            // Auto-detect editbox list if no mapping is provided.
            if editbox::maybe_dispatch(ctx, form_id, args)? {
                return Ok(true);
            }

            // Generic list primitives (flags, name-lists, etc.).
            if codes::GLOBAL_INT_LIST_FORMS.contains(&form_id) {
                return int_list::dispatch(ctx, form_id, args);
            }
            if codes::GLOBAL_STR_LIST_FORMS.contains(&form_id) {
                return str_list::dispatch(ctx, form_id, args);
            }

            // Counters.
            if form_id == codes::FORM_GLOBAL_COUNTER {
                return counter::dispatch(ctx, form_id, args);
            }

            // FRAME_ACTION: treat as a generic int list for bring-up.
            if form_id == codes::FORM_GLOBAL_FRAME_ACTION {
                return int_list::dispatch(ctx, form_id, args);
            }

            // Last-resort stub to keep the VM running.
            stub::dispatch(ctx, form_id, args)
        }
    }
}
