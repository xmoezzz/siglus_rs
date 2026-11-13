//! Oldest split-file layout: 15-code elements, scalar cameras, and 20-byte groups.
//! Keep its wire records separate from later generations; only primitives and
//! records with identical field order are shared.
use super::*;
use anyhow::Context;

impl<'a> SceneVm<'a> {
    pub(super) fn normalize_short_call_returns(&self, frames: &mut [CallFrame]) {
        // This generation writes ret_form_code on the callee at GOSUB,
        // FARCALL and USER_CMD entry. Later engines (and our runtime) keep
        // it on the caller. Lexer return positions already belong to the
        // caller in both generations and must stay on their original frame.
        for index in 1..frames.len() {
            frames[index - 1].ret_form = frames[index].ret_form;
        }
        if let Some(active) = frames.last_mut() {
            active.ret_form = self.cfg.fm_void;
        }
    }

    pub(super) fn read_short_msg_back(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::MsgBackState> {
        let cnt = rd.count(41)?;
        let mut st = runtime::globals::MsgBackState::default();
        st.history.clear();
        for _ in 0..cnt {
            let mut entry = runtime::globals::MsgBackEntry::default();
            entry.pct_flag = rd.bool()?;
            entry.msg_str = rd.string()?;
            entry.original_name = rd.string()?;
            // This layout stores one speaker name.
            entry.disp_name = entry.original_name.clone();
            entry.pct_pos_x = rd.i32()?;
            entry.pct_pos_y = rd.i32()?;
            entry.koe_no_list = rd.extend_i32_list()?;
            entry.chr_no_list = rd.extend_i32_list()?;
            entry.koe_play_no = rd.i32()? as i64;
            entry.debug_msg = rd.string()?;
            entry.scn_no = rd.i32()? as i64;
            entry.line_no = rd.i32()? as i64;
            st.history.push(entry);
        }
        st.history_cnt = cnt;
        st.history_cnt_max = cnt.max(256);
        st.history_start_pos = rd.i32()?.max(0) as usize;
        st.history_last_pos = rd.i32()?.max(0) as usize;
        st.history_insert_pos = rd.i32()?.max(0) as usize;
        st.new_msg_flag = rd.bool()?;
        if st.history.len() < st.history_cnt_max {
            st.history
                .resize_with(st.history_cnt_max, runtime::globals::MsgBackEntry::default);
        }
        Ok(st)
    }

    pub(super) fn read_short_sound(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<SavedSound> {
        let bgm_regist_name = rd.string()?;
        let bgm_volume = rd.i32()?.clamp(0, 255) as u8;
        let bgm_delay_time = 0;
        let bgm_loop_flag = rd.bool()?;
        let bgm_pause_flag = false;
        let koe_volume = rd.i32()?.clamp(0, 255) as u8;
        let pcm_volume = rd.i32()?.clamp(0, 255) as u8;
        let pcmch = rd.fixed_items(|rd| Self::read_short_pcmch(rd))?;
        let se_volume = rd.i32()?.clamp(0, 255) as u8;
        let mov_file_name = rd.string()?;
        Ok(SavedSound {
            bgm_regist_name,
            bgm_volume,
            bgm_delay_time,
            bgm_loop_flag,
            bgm_pause_flag,
            koe_volume,
            pcm_volume,
            pcmch,
            se_volume,
            mov_file_name,
        })
    }

    fn read_short_pcmch(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::PcmChPersistentState> {
        Ok(runtime::globals::PcmChPersistentState {
            pcm_name: rd.string()?,
            bgm_name: String::new(),
            koe_no: rd.i32()? as i64,
            se_no: rd.i32()? as i64,
            volume_type: rd.i32()? as i64,
            chara_no: rd.i32()? as i64,
            volume: rd.i32()? as i64,
            delay_time: 0,
            fade_in_time: 0,
            loop_flag: rd.bool()?,
            bgm_fade_target_flag: rd.bool()?,
            bgm_fade2_target_flag: rd.bool()?,
            bgm_fade_source_flag: rd.bool()?,
            ready_flag: false,
        })
    }

    pub(super) fn read_short_stage(
        &mut self,
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        stage_idx: i64,
    ) -> Result<(
        runtime::globals::StageFormState,
        runtime::globals::BtnSelectRuntimeState,
    )> {
        let mut st = runtime::globals::StageFormState {
            initialized_from_gameexe: true,
            ..Default::default()
        };

        st.group_lists
            .insert(stage_idx, rd.fixed_items(Self::read_short_group)?);
        let mut slot = 0;
        let objects = rd.fixed_items(|rd| {
            // Native C_elm_object_list::_save omits disabled/non-saving slots
            // without changing the declared array length. Children have no INI filter.
            let saved = self.native_object_slot_saved(slot);
            slot += 1;
            if saved {
                Self::read_short_object(rd)
            } else {
                Ok(Default::default())
            }
        })?;
        st.object_lists.insert(stage_idx, objects);
        let mut windows = rd.fixed_items(Self::read_short_mwnd)?;
        for m in &mut windows {
            if let Some(waku) = m
                .msg_waku_no
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| self.ctx.tables.waku_templates.get(n))
            {
                m.waku_file = waku.waku_file.clone();
                m.filter_file = waku.filter_file.clone();
            }
        }
        st.mwnd_lists.insert(stage_idx, windows);
        let btn_select = Self::read_short_btn_select(rd)?;
        st.btn_select_states.insert(stage_idx, btn_select.clone());
        st.effect_lists
            .insert(stage_idx, rd.fixed_items(Self::read_cpp_effect)?);
        st.quake_lists
            .insert(stage_idx, rd.fixed_items(Self::read_cpp_quake)?);
        Ok((st, btn_select))
    }

    pub(super) fn native_object_slot_saved(&self, slot: usize) -> bool {
        let mut use_flag = true;
        let mut save_flag = true;
        if let Some(cfg) = self.ctx.tables.gameexe.as_ref() {
            for entry in &cfg.entries {
                let [prefix, index, field] = entry.key_parts.as_slice() else {
                    continue;
                };
                if prefix != "OBJECT" {
                    continue;
                }
                let (start, end) = index.split_once('-').unwrap_or((index, index));
                let (Ok(start), Ok(end)) =
                    (start.trim().parse::<usize>(), end.trim().parse::<usize>())
                else {
                    continue;
                };
                if !(start..=end).contains(&slot) {
                    continue;
                }
                let Some(value) = entry.item_unquoted(0).and_then(|v| v.parse::<i32>().ok()) else {
                    continue;
                };
                match field.as_str() {
                    "USE" => use_flag = value != 0,
                    "SAVE" => save_flag = value != 0,
                    _ => {}
                }
            }
        }
        use_flag && save_flag
    }

    pub(super) fn read_short_mwnd(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::MwndState> {
        let mut m = runtime::globals::MwndState::default();
        m.order = rd.i32()? as i64;
        m.layer = rd.i32()? as i64;
        m.world = rd.i32()? as i64;
        m.novel_mode = rd.i32()? as i64;
        m.mwnd_extend_type = rd.i32()? as i64;
        m.window_pos = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.window_size = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.message_pos = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.message_margin = Some((
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        ));
        m.name_disp_mode = rd.i32()? as i64;
        m.name_bracket = rd.i32()? as i64;
        m.name_extend_type = rd.i32()? as i64;
        m.name_window_pos = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_window_size = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_window_rect = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        m.name_message_pos = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_message_margin = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        m.overflow_check_size = rd.i32()? as i64;
        m.open_anime_type = rd.i32()? as i64;
        m.open_anime_time = rd.i32()? as i64;
        m.close_anime_type = rd.i32()? as i64;
        m.close_anime_time = rd.i32()? as i64;
        m.time = rd.i32()? as i64;
        m.msg_block_started = rd.bool()?;
        m.window_appear = rd.bool()?;
        m.open = m.window_appear;
        m.name_appear = rd.bool()?;
        m.clear_ready = rd.bool()?;
        m.auto_mode_end_moji_cnt = rd.i32()? as i64;
        m.target_msg_no = rd.i32()? as i64;
        m.slide_msg = rd.bool()?;
        m.slide_time = rd.i32()? as i64;
        let koe_no = rd.i32()? as i64;
        m.koe_play_flag = rd.bool()?;
        if m.koe_play_flag {
            m.koe = Some((koe_no, 0));
        }
        m.open_anime_type = rd.i32()? as i64;
        m.open_anime_time = rd.i32()? as i64;
        m.open_anime_start_time = rd.i32()? as i64;
        m.close_anime_type = rd.i32()? as i64;
        m.close_anime_time = rd.i32()? as i64;
        m.close_anime_start_time = rd.i32()? as i64;

        let message_count = rd.count(4)?;
        let mut pages = Vec::with_capacity(message_count.max(1));
        for _ in 0..message_count {
            pages.push(Self::read_short_mwnd_message(rd, &mut m)?);
        }
        let active_index = m
            .target_msg_no
            .clamp(0, pages.len().saturating_sub(1) as i64) as usize;
        if let Some(active) = pages.get(active_index).cloned() {
            m.message_pages = pages[..active_index].to_vec();
            m.msg_text = active.msg_text;
            m.glyphs = active.glyphs;
            m.disp_moji_cnt = active.disp_moji_cnt;
            m.hide_moji_cnt = active.hide_moji_cnt;
            m.cur_msg_type = active.cur_msg_type;
            m.cur_msg_type_decided = active.cur_msg_type_decided;
            m.ruby_start_pos = active.ruby_start_pos;
            m.ruby_start_ready = active.ruby_start_ready;
            m.cursor_pos = active.cursor_pos;
            m.moji_rep_pos = active.moji_rep_pos;
            m.indent_pos = active.indent_pos;
            m.indent_moji = active.indent_moji;
            m.indent_count = active.indent_count;
            m.line_head = active.line_head;
            m.ruby_pending = active.ruby_pending;
            m.moji_size = active.moji_size;
            m.moji_color = active.moji_color;
            m.shadow_color = active.shadow_color;
            m.fuchi_color = active.fuchi_color;
            m.chara_moji_color = active.chara_moji_color;
            m.chara_shadow_color = active.chara_shadow_color;
            m.chara_fuchi_color = active.chara_fuchi_color;
            m.msgbtn = active.msgbtn;
        }
        let mut reveal_index = 0usize;
        for page in &mut m.message_pages {
            for glyph in &mut page.glyphs {
                if !glyph.ruby {
                    reveal_index += 1;
                }
                glyph.reveal_index = reveal_index.max(1);
            }
        }
        for glyph in &mut m.glyphs {
            if !glyph.ruby {
                reveal_index += 1;
            }
            glyph.reveal_index = reveal_index.max(1);
        }
        Self::read_short_mwnd_waku(rd, &mut m, false)?;
        Self::read_cpp_mwnd_name(rd, &mut m)?;
        Self::read_short_mwnd_waku(rd, &mut m, true)?;
        Self::read_cpp_mwnd_selection(rd, &mut m)?;
        Ok(m)
    }

    pub(super) fn read_short_mwnd_message(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
    ) -> Result<runtime::globals::MwndMessagePageState> {
        let cnt_x = rd.i32()? as i64;
        let cnt_y = rd.i32()? as i64;
        let pos_x = rd.i32()? as i64;
        let pos_y = rd.i32()? as i64;
        let rep_x = rd.i32()? as i64;
        let rep_y = rd.i32()? as i64;
        let moji_size = rd.i32()? as i64;
        let space_x = rd.i32()? as i64;
        let space_y = rd.i32()? as i64;
        let moji_color = rd.i32()? as i64;
        let shadow_color = rd.i32()? as i64;
        let fuchi_color = -1;
        let ruby_size = rd.i32()? as i64;
        let ruby_space = rd.i32()? as i64;
        let talk_l = rd.i32()? as i64;
        let talk_t = rd.i32()? as i64;
        let talk_r = rd.i32()? as i64;
        let talk_b = rd.i32()? as i64;
        m.window_moji_cnt = Some((cnt_x, cnt_y));
        m.message_pos = Some((pos_x, pos_y));
        m.moji_space = Some((space_x, space_y));
        m.message_margin = Some((talk_l, talk_t, talk_r, talk_b));
        m.ruby_size = ruby_size;
        m.ruby_space = ruby_space;

        let chara_moji = rd.i32()? as i64;
        let chara_shadow = rd.i32()? as i64;
        let chara_fuchi = -1;
        let indent_pos = rd.i32()? as i64;
        let indent_u16 = rd.u16()?;
        let indent_count = rd.i32()? as i64;
        let cur_msg_type = rd.i32()? as i64;
        let cur_msg_type_decided = rd.bool()?;
        let line_head = rd.bool()?;
        let ruby_x = rd.i32()? as i64;
        let ruby_y = rd.i32()? as i64;
        let ruby_start_ready = rd.bool()?;
        let disp_moji_cnt = rd.i32()? as i64;
        let hide_moji_cnt = rd.i32()? as i64;
        let debug_msg = rd.string()?;
        let ruby = rd.string()?;
        let mut glyphs = rd.extend_items(Self::read_early_mwnd_glyph)?;
        let mut body_index = 0usize;
        for glyph in &mut glyphs {
            if !glyph.ruby {
                body_index += 1;
            }
            glyph.reveal_index = body_index.max(1);
        }
        let msg_text = if !debug_msg.is_empty() {
            debug_msg
        } else {
            let units: Vec<u16> = glyphs
                .iter()
                .filter(|glyph| glyph.moji_type == 0 && !glyph.ruby)
                .map(|glyph| glyph.code as u16)
                .collect();
            String::from_utf16_lossy(&units)
        };
        Ok(runtime::globals::MwndMessagePageState {
            msg_text,
            glyphs,
            disp_moji_cnt,
            hide_moji_cnt,
            cur_msg_type,
            cur_msg_type_decided,
            ruby_start_pos: (ruby_x, ruby_y),
            ruby_start_ready,
            cursor_pos: (pos_x, pos_y),
            moji_rep_pos: (rep_x, rep_y),
            indent_pos,
            indent_moji: (indent_u16 != 0)
                .then(|| char::from_u32(indent_u16 as u32).unwrap_or('\u{fffd}')),
            indent_count,
            line_head,
            ruby_pending: (!ruby.is_empty()).then_some(runtime::globals::MwndRubyPendingState {
                text: ruby,
                start_pos: Some((ruby_x, ruby_y)),
            }),
            moji_size: Some(moji_size),
            moji_color: (moji_color >= 0).then_some(moji_color),
            shadow_color: (shadow_color >= 0).then_some(shadow_color),
            fuchi_color: (fuchi_color >= 0).then_some(fuchi_color),
            chara_moji_color: (chara_moji >= 0).then_some(chara_moji),
            chara_shadow_color: (chara_shadow >= 0).then_some(chara_shadow),
            chara_fuchi_color: (chara_fuchi >= 0).then_some(chara_fuchi),
            msgbtn: None,
        })
    }

    fn read_short_mwnd_waku(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
        name_waku: bool,
    ) -> Result<()> {
        let template_no = rd.i32()? as i64;
        let margin = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        let color = rd.take_raw(4)?;
        let filter_config_color = rd.bool()?;
        let filter_config_tr = rd.bool()?;
        rd.skip(2)?;
        let _key_template = rd.i32()?;
        let key_mode = rd.i32()? as i64;
        let key_x = rd.i32()? as i64;
        let key_y = rd.i32()? as i64;
        let faces = rd.fixed_items(Self::read_cpp_object)?;
        let objects = rd.fixed_items(Self::read_cpp_object)?;
        if !name_waku {
            m.msg_waku_no = Some(template_no);
            m.filter_margin = Some(margin);
            m.filter_color = Some((color[3], color[2], color[1], color[0]));
            m.filter_config_color = filter_config_color;
            m.filter_config_tr = filter_config_tr;
            m.key_icon_mode = key_mode;
            m.key_icon_pos = Some((key_x, key_y));
            m.face_list = faces;
            m.object_list = objects;
        }
        Ok(())
    }

    pub(super) fn read_short_effect(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::ScreenEffectState> {
        let mut e = runtime::globals::ScreenEffectState {
            x: Self::read_cpp_int_event_raw(rd)?,
            y: Self::read_cpp_int_event_raw(rd)?,
            z: Self::read_cpp_int_event_raw(rd)?,
            mono: Self::read_cpp_int_event_raw(rd)?,
            reverse: Self::read_cpp_int_event_raw(rd)?,
            bright: Self::read_cpp_int_event_raw(rd)?,
            dark: Self::read_cpp_int_event_raw(rd)?,
            color_r: Self::read_cpp_int_event_raw(rd)?,
            color_g: Self::read_cpp_int_event_raw(rd)?,
            color_b: Self::read_cpp_int_event_raw(rd)?,
            color_rate: Self::read_cpp_int_event_raw(rd)?,
            color_add_r: Self::read_cpp_int_event_raw(rd)?,
            color_add_g: Self::read_cpp_int_event_raw(rd)?,
            color_add_b: Self::read_cpp_int_event_raw(rd)?,
            begin_order: rd.i32()?,
            end_order: rd.i32()?,
            begin_layer: rd.i32()?,
            end_layer: rd.i32()?,
            ..Default::default()
        };

        Ok(e)
    }

    fn read_short_btn_select(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::BtnSelectRuntimeState> {
        let mut selection = runtime::globals::BtnSelectRuntimeState {
            template_no: rd.i32()? as i64,
            ..Default::default()
        };

        rd.skip(26 * 4)?;
        selection.appear_flag = rd.bool()?;
        selection.processing_flag_0 = rd.bool()?;
        selection.processing_flag_1 = rd.bool()?;
        let count = rd.count(4)?;
        anyhow::ensure!(
            count == 0,
            "active short-element button selection saves are not supported"
        );
        Ok(selection)
    }

    pub(super) fn read_short_group(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::GroupState> {
        let mut g = runtime::globals::GroupState {
            order: rd.i32()? as i64,
            layer: rd.i32()? as i64,
            cancel_se_no: rd.i32()? as i64,
            decided_button_no: rd.i32()? as i64,
            started: rd.bool()?,
            wait_flag: rd.bool()?,
            cancel_flag: rd.bool()?,
            ..Default::default()
        };

        rd.skip(1)?;
        Ok(g)
    }

    pub(super) fn read_short_object(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::ObjectState> {
        let mut obj = runtime::globals::ObjectState {
            object_type: rd.i32()? as i64,
            ..Default::default()
        };

        obj.base.wipe_copy = rd.i32()? as i64;
        obj.base.wipe_erase = rd.i32()? as i64;

        obj.rect_param.left = rd.i32()? as i64;
        obj.rect_param.top = rd.i32()? as i64;
        obj.rect_param.right = rd.i32()? as i64;
        obj.rect_param.bottom = rd.i32()? as i64;
        obj.rect_param.color_argb = rd.i32()? as i64;
        obj.string_param.moji_size = rd.i32()? as i64;
        obj.string_param.moji_space_x = rd.i32()? as i64;
        obj.string_param.moji_space_y = rd.i32()? as i64;
        obj.string_param.moji_cnt = rd.i32()? as i64;
        obj.string_param.moji_color = rd.i32()? as i64;
        obj.string_param.shadow_color = rd.i32()? as i64;
        obj.string_param.shadow_mode = rd.i32()? as i64;
        obj.number_value = rd.i32()? as i64;
        obj.number_param.keta_max = rd.i32()? as i64;
        obj.number_param.disp_zero = rd.i32()? as i64;
        obj.number_param.disp_sign = rd.i32()? as i64;
        obj.number_param.tumeru_sign = rd.i32()? as i64;
        obj.number_param.space_mod = rd.i32()? as i64;
        obj.number_param.space = rd.i32()? as i64;
        anyhow::ensure!(
            obj.object_type != 4,
            "unsupported short-element weather object save"
        );
        rd.skip(72)?;
        obj.thumb_save_no = rd.i32()? as i64;
        obj.movie.loop_flag = rd.bool()?;
        obj.movie.auto_free_flag = rd.bool()?;
        obj.movie.real_time_flag = rd.bool()?;
        obj.movie.pause_flag = rd.bool()?;
        {
            // Button work is also unconditional; no presence word.
            obj.button.sys_type = rd.i32()? as i64;
            obj.button.sys_type_opt = rd.i32()? as i64;
            obj.button.action_no = rd.i32()? as i64;
            obj.button.se_no = rd.i32()? as i64;
            obj.button.button_no = rd.i32()? as i64;
            obj.button.group_no = rd.i32()? as i64;
            obj.button.enabled = obj.button.action_no >= 0;
            obj.button.push_keep = rd.i32()? != 0;
            obj.button.state = rd.i32()? as i64;
            obj.button.mode = rd.i32()? as i64;
            obj.button.cut_no = rd.i32()? as i64;
            let _ = rd.i32()?;
            let _ = rd.i32()?;
            obj.button.decided_action_z_no = rd.i32()? as i64;
        }
        obj.base.disp = rd.i32()? as i64;
        // Mirror of the writer: original pat_no is a full C_elm_int_event.
        obj.runtime.prop_events.patno = Self::read_cpp_int_event_raw(rd)?;
        if obj.runtime.prop_events.patno.loop_type == -1 {
            obj.base.patno = obj.runtime.prop_events.patno.value as i64;
        }
        obj.base.order = rd.i32()? as i64;
        obj.base.layer = rd.i32()? as i64;
        obj.base.world = rd.i32()? as i64;

        obj.runtime.prop_events.x = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.y = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.z = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.center_x = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.center_y = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.center_z = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.center_rep_x = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.center_rep_y = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.center_rep_z = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.scale_x = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.scale_y = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.scale_z = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.rotate_x = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.rotate_y = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.rotate_z = Self::read_cpp_int_event_raw(rd)?;
        obj.base.clip_use = rd.i32()? as i64;
        obj.runtime.prop_events.clip_left = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.clip_top = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.clip_right = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.clip_bottom = Self::read_cpp_int_event_raw(rd)?;
        obj.base.src_clip_use = rd.i32()? as i64;
        obj.runtime.prop_events.src_clip_left = runtime::int_event::IntEvent::new(rd.i32()?);
        obj.runtime.prop_events.src_clip_top = runtime::int_event::IntEvent::new(rd.i32()?);
        obj.runtime.prop_events.src_clip_right = runtime::int_event::IntEvent::new(rd.i32()?);
        obj.runtime.prop_events.src_clip_bottom = runtime::int_event::IntEvent::new(rd.i32()?);
        obj.runtime.prop_events.tr = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.mono = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.reverse = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.bright = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.dark = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_r = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_g = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_b = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_rate = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_add_r = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_add_g = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.color_add_b = Self::read_cpp_int_event_raw(rd)?;

        obj.base.light_no = rd.i32()? as i64;

        obj.base.blend = rd.i32()? as i64;

        obj.runtime.prop_event_lists.x_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.y_rep = Self::read_cpp_int_event_extend_list(rd)?;

        obj.runtime.prop_event_lists.tr_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_lists.f = rd.extend_i32_list()?;
        let file_name = rd.string().context("short object file name")?;
        obj.file_name = if file_name.is_empty() {
            None
        } else {
            Some(file_name)
        };
        let string_value = rd.string().context("short object text")?;
        obj.string_value = if string_value.is_empty() {
            None
        } else {
            Some(string_value)
        };
        // This layout includes both callback name strings.
        {
            obj.button.decided_action_scn_name = rd.string()?;
            obj.button.decided_action_cmd_name = rd.string()?;
        }
        obj.frame_action = Self::read_cpp_frame_action(rd)?;
        obj.frame_action_ch = rd.extend_items(|rd| Self::read_cpp_frame_action(rd))?;
        let gan_file = rd.string()?;
        obj.gan_file = if gan_file.is_empty() {
            None
        } else {
            Some(gan_file)
        };
        obj.gan.read_short_work(rd)?;
        // Children use a count-only extend list.

        obj.runtime.child_objects = rd.extend_items(Self::read_short_object)?;
        obj.used = obj.object_type != 0 || obj.file_name.is_some() || obj.string_value.is_some();
        Ok(obj)
    }

    pub(super) fn read_short_world(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        world_no: i32,
    ) -> Result<runtime::globals::WorldState> {
        let mut world = runtime::globals::WorldState::new(world_no);
        world.mode = rd.i32()?;
        world.camera_eye_x = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_eye_y = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_eye_z = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_pint_x = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_pint_y = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_pint_z = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_up_x = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_up_y = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_up_z = runtime::int_event::IntEvent::new(rd.i32()?);
        world.camera_view_angle = rd.i32()?;
        world.mono = rd.i32()?;
        Ok(world)
    }

    pub(super) fn read_short_local_settings(
        &mut self,
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<()> {
        let button_count = self.mwnd_waku_btn_count();
        let script = &mut self.ctx.globals.script;
        let syscom = &mut self.ctx.globals.syscom;
        script.cur_koe_no = rd.i32()? as i64;
        script.cur_chr_no = rd.i32()? as i64;
        script.cur_read_flag_scn_no = rd.i32()? as i64;
        script.cur_read_flag_flag_no = rd.i32()? as i64;
        syscom.current_save_scene_title = rd.string()?;
        syscom.current_save_full_message.clear();
        script.cursor_no = 0;
        syscom.syscom_menu_disable = rd.bool()?;
        script.hide_mwnd_disable = rd.bool()?;
        syscom.mwnd_btn_disable_all = false;
        syscom.mwnd_btn_disable.clear();
        for idx in 0..button_count {
            if rd.bool()? {
                syscom.mwnd_btn_disable.insert(idx as i64, true);
            }
        }
        syscom.mwnd_btn_touch_disable = rd.bool()?;
        script.skip_disable = rd.bool()?;
        script.ctrl_disable = rd.bool()?;
        script.not_stop_skip_by_click = rd.bool()?;
        script.not_skip_msg_by_click = rd.bool()?;
        script.skip_unread_message = rd.bool()?;
        script.shortcut_disable = rd.bool()?;
        script.msg_back_off = false;
        script.msg_back_disp_off = false;
        script.auto_mode_flag = rd.bool()?;
        script.auto_mode_moji_wait = -1;
        script.auto_mode_min_wait = -1;
        script.msg_speed = rd.i32()? as i64;
        script.msg_nowait = rd.bool()?;
        script.async_msg_mode = rd.bool()?;
        script.async_msg_mode_once = rd.bool()?;
        script.multi_msg_mode = false;
        script.skip_trigger = false;
        script.cursor_disp_off = rd.bool()?;
        script.cursor_runtime_visible = !script.cursor_disp_off;
        script.cursor_move_by_key_disable = rd.bool()?;
        script.key_disable.clear();
        for key in 0u16..=255 {
            if rd.bool()? {
                script.key_disable.insert(key as u8);
            }
        }
        script.mwnd_anime_on_flag = rd.bool()?;
        script.mwnd_anime_off_flag = rd.bool()?;
        script.mwnd_disp_off_flag = rd.bool()?;
        script.koe_dont_stop_on_flag = rd.bool()?;
        script.koe_dont_stop_off_flag = rd.bool()?;
        script.quake_stop_flag = rd.bool()?;
        self.ctx.globals.cg_table_off = rd.bool()?;
        script.bgmfade_flag = rd.bool()?;
        script.dont_set_save_point = false;
        script.ignore_r_flag = false;
        script.wait_display_vsync_off_flag = false;
        // These fields did not exist in this generation.
        script.font_name.clear();
        script.font_bold = -1;
        script.font_shadow = -1;
        script.msg_back_disable = false;
        script.auto_mode_moji_cnt = 0;
        script.mouse_cursor_hide_onoff = -1;
        script.mouse_cursor_hide_time = -1;
        script.msg_back_save_cntr = 0;
        script.emote_mouth_stop_flag = false;
        script.time_stop_flag = false;
        script.counter_time_stop_flag = false;
        script.frame_action_time_stop_flag = false;
        script.stage_time_stop_flag = false;
        syscom.replay_koe =
            (script.cur_koe_no >= 0).then_some((script.cur_koe_no, script.cur_chr_no));
        Ok(())
    }
}
