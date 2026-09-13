//! Native saves from the early engine generation (3116-byte file header,
//! 332-byte local POD). Field order here follows that generation's stream;
//! keep newer fields in the current readers in vm.rs.
//!
//! Shared records (arrays, events, frame actions, window frames) still use the
//! common readers. Records whose wire layout changed have explicit variants.

use super::*;

impl<'a> SceneVm<'a> {
    pub(super) fn read_early_local_data_pod(
        &mut self,
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<()> {
        let script = &mut self.ctx.globals.script;

        script.cur_koe_no = rd.i32()? as i64;
        script.cur_chr_no = rd.i32()? as i64;
        script.cur_read_flag_scn_no = rd.i32()? as i64;
        script.cur_read_flag_flag_no = rd.i32()? as i64;
        script.cursor_no = rd.i32()? as i64;

        self.ctx.globals.syscom.syscom_menu_disable = rd.bool()?;
        script.hide_mwnd_disable = rd.bool()?;
        script.msg_back_disable = rd.bool()?;
        script.shortcut_disable = rd.bool()?;

        script.skip_disable = rd.bool()?;
        script.ctrl_disable = rd.bool()?;
        script.not_stop_skip_by_click = rd.bool()?;
        script.not_skip_msg_by_click = rd.bool()?;
        script.skip_unread_message = rd.bool()?;
        script.auto_mode_flag = rd.bool()?;
        rd.skip(2)?;
        script.auto_mode_moji_wait = rd.i32()? as i64;
        script.auto_mode_min_wait = rd.i32()? as i64;
        script.auto_mode_moji_cnt = rd.i32()? as i64;
        script.mouse_cursor_hide_onoff = -1;
        script.mouse_cursor_hide_time = -1;
        script.msg_back_save_cntr = 0;

        script.msg_speed = rd.i32()? as i64;
        script.msg_nowait = rd.bool()?;
        script.async_msg_mode = rd.bool()?;
        script.async_msg_mode_once = rd.bool()?;
        script.multi_msg_mode = rd.bool()?;
        script.skip_trigger = rd.bool()?;
        script.koe_dont_stop_on_flag = rd.bool()?;
        script.koe_dont_stop_off_flag = rd.bool()?;

        self.ctx.globals.syscom.mwnd_btn_disable_all = rd.bool()?;
        self.ctx.globals.syscom.mwnd_btn_touch_disable = rd.bool()?;
        script.mwnd_anime_on_flag = rd.bool()?;
        script.mwnd_anime_off_flag = rd.bool()?;
        script.mwnd_disp_off_flag = rd.bool()?;

        script.msg_back_off = rd.bool()?;
        script.msg_back_disp_off = rd.bool()?;
        script.font_bold = -1;
        script.font_shadow = -1;

        script.cursor_disp_off = rd.bool()?;
        script.cursor_runtime_visible = !script.cursor_disp_off;
        script.cursor_move_by_key_disable = rd.bool()?;
        script.key_disable.clear();
        for key in 0u16..=255u16 {
            if rd.bool()? {
                script.key_disable.insert(key as u8);
            }
        }

        script.quake_stop_flag = rd.bool()?;
        script.emote_mouth_stop_flag = rd.bool()?;
        self.ctx.globals.cg_table_off = rd.bool()?;
        script.bgmfade_flag = rd.bool()?;
        script.dont_set_save_point = rd.bool()?;
        script.ignore_r_flag = rd.bool()?;
        script.wait_display_vsync_off_flag = rd.bool()?;

        script.time_stop_flag = rd.bool()?;
        script.counter_time_stop_flag = rd.bool()?;
        script.frame_action_time_stop_flag = rd.bool()?;
        script.stage_time_stop_flag = rd.bool()?;
        rd.skip(1)?;

        self.ctx.globals.syscom.replay_koe = if script.cur_koe_no >= 0 {
            Some((script.cur_koe_no, script.cur_chr_no))
        } else {
            None
        };
        Ok(())
    }

    pub(super) fn read_early_object(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::ObjectState> {
        let mut obj = runtime::globals::ObjectState::default();
        obj.object_type = rd.i32()? as i64;
        obj.base.wipe_copy = rd.i32()? as i64;
        obj.base.wipe_erase = rd.i32()? as i64;
        obj.base.click_disable = rd.i32()? as i64;
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
        obj.number_param.space = rd.i32()? as i64;
        {
            // Weather work is present for every object type.
            obj.weather_param.weather_type = rd.i32()? as i64;
            obj.weather_param.cnt = rd.i32()? as i64;
            obj.weather_param.pat_mode = rd.i32()? as i64;
            obj.weather_param.pat_no_00 = rd.i32()? as i64;
            obj.weather_param.pat_no_01 = rd.i32()? as i64;
            obj.weather_param.pat_time = rd.i32()? as i64;
            obj.weather_param.move_time_x = rd.i32()? as i64;
            obj.weather_param.move_time_y = rd.i32()? as i64;
            obj.weather_param.sin_time_x = rd.i32()? as i64;
            obj.weather_param.sin_time_y = rd.i32()? as i64;
            obj.weather_param.sin_power_x = rd.i32()? as i64;
            obj.weather_param.sin_power_y = rd.i32()? as i64;
            obj.weather_param.center_x = rd.i32()? as i64;
            obj.weather_param.center_y = rd.i32()? as i64;
            obj.weather_param.center_rotate = rd.i32()? as i64;
            obj.weather_param.appear_range = rd.i32()? as i64;
            obj.weather_param.zoom_min = rd.i32()? as i64;
            obj.weather_param.zoom_max = rd.i32()? as i64;
            obj.weather_param.scale_x = rd.i32()? as i64;
            obj.weather_param.scale_y = rd.i32()? as i64;
            obj.weather_param.active_time = rd.i32()? as i64;
            obj.weather_param.real_time_flag = rd.bool()?;
            // MSVC pads the trailing bool to the struct's 4-byte alignment.
            rd.skip(3)?;
            // `move_time` is a Rust convenience alias for TYPE_B; it is not an
            // additional C++ serialized field.
            obj.weather_param.move_time = obj.weather_param.move_time_x;
        }
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
            let group = rd.element()?;
            obj.button.enabled = !group.is_empty();
            obj.button.push_keep = rd.i32()? != 0;
            obj.button.state = rd.i32()? as i64;
            obj.button.mode = rd.i32()? as i64;
            obj.button.cut_no = rd.i32()? as i64;
            let _ = rd.i32()?;
            let _ = rd.i32()?;
            obj.button.decided_action_z_no = rd.i32()? as i64;
            obj.button.alpha_test = rd.i32()? != 0;
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
        obj.base.child_sort_type = rd.i32()? as i64;
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
        obj.runtime.prop_events.src_clip_left = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.src_clip_top = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.src_clip_right = Self::read_cpp_int_event_raw(rd)?;
        obj.runtime.prop_events.src_clip_bottom = Self::read_cpp_int_event_raw(rd)?;
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
        obj.base.mask_no = rd.i32()? as i64;
        obj.base.tonecurve_no = rd.i32()? as i64;
        obj.base.light_no = rd.i32()? as i64;
        obj.base.fog_use = rd.i32()? as i64;
        obj.base.culling = rd.i32()? as i64;
        obj.base.alpha_test = rd.i32()? as i64;
        obj.base.alpha_blend = rd.i32()? as i64;
        obj.base.blend = rd.i32()? as i64;
        let _flags = rd.i32()?;
        obj.runtime.prop_event_lists.x_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.y_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.z_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.tr_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_lists.f = rd.extend_i32_list()?;
        let file_name = rd.string()?;
        obj.file_name = if file_name.is_empty() {
            None
        } else {
            Some(file_name)
        };
        let string_value = rd.string()?;
        obj.string_value = if string_value.is_empty() {
            None
        } else {
            Some(string_value)
        };
        obj.button.decided_action_scn_name = rd.string()?;
        obj.button.decided_action_cmd_name = rd.string()?;
        obj.frame_action = Self::read_cpp_frame_action(rd)?;
        obj.frame_action_ch = rd.extend_items(|rd| Self::read_cpp_frame_action(rd))?;
        let gan_file = rd.string()?;
        obj.gan_file = if gan_file.is_empty() {
            None
        } else {
            Some(gan_file)
        };
        obj.gan.read_original_work(rd)?;
        obj.runtime.child_objects = rd.extend_items(|rd| Self::read_early_object(rd))?;
        obj.used = obj.object_type != 0 || obj.file_name.is_some() || obj.string_value.is_some();
        Ok(obj)
    }

    pub(super) fn read_early_mwnd_glyph(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::MwndGlyphState> {
        let moji_type = rd.i32()?;
        let code = rd.i32()?;
        let size = rd.i32()? as i64;
        let moji_color_no = rd.i32()? as i64;
        let shadow_color_no = rd.i32()? as i64;
        let fuchi_color_no = -1;
        let x = rd.i32()? as i64;
        let y = rd.i32()? as i64;
        let appeared = rd.bool()?;
        let ruby = rd.bool()?;
        let ch = if moji_type == 0 {
            char::from_u32(code as u32).unwrap_or('\u{fffd}')
        } else {
            '\0'
        };
        Ok(runtime::globals::MwndGlyphState {
            moji_type,
            code,
            ch,
            x,
            y,
            size,
            moji_color_no,
            shadow_color_no,
            fuchi_color_no,
            shadow: shadow_color_no >= 0,
            fuchi: fuchi_color_no >= 0,
            bold: false,
            reveal_index: 1,
            ruby,
            appeared,
            message_button: None,
        })
    }

    pub(super) fn read_early_mwnd_message(
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
        let _name_moji_size = rd.i32()?;
        let _name_newline = rd.i32()?;
        let _name_bracket = rd.i32()?;
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

    pub(super) fn read_early_mwnd_name(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
    ) -> Result<()> {
        let _template_no = rd.i32()?;
        let pos_x = rd.i32()? as i64;
        let pos_y = rd.i32()? as i64;
        let _size = rd.i32()?;
        let _space_x = rd.i32()?;
        let _space_y = rd.i32()?;
        let _cnt = rd.i32()?;
        m.name_window_align = rd.i32()? as i64;
        let name_moji_color = rd.i32()? as i64;
        let name_shadow_color = rd.i32()? as i64;
        let name_fuchi_color = -1;
        m.name_moji_color = (name_moji_color >= 0).then_some(name_moji_color);
        m.name_shadow_color = (name_shadow_color >= 0).then_some(name_shadow_color);
        m.name_fuchi_color = (name_fuchi_color >= 0).then_some(name_fuchi_color);
        m.name_text = rd.string()?;
        m.name_message_pos = (pos_x, pos_y);
        m.name_window_rect = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        m.name_glyphs = rd.extend_items(Self::read_early_mwnd_glyph)?;
        Ok(())
    }

    pub(super) fn read_early_mwnd_selection(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
    ) -> Result<()> {
        let _template = rd.i32()?;
        for _ in 0..9 {
            let _ = rd.i32()?;
        }
        let disp_item_count = rd.i32()?.max(0) as usize;
        let cancel_enable = rd.bool()?;
        let count = rd.i32()?.max(0) as usize;
        let mut choices = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = rd.i32()? as i64;
            let x = rd.i32()? as i64;
            let y = rd.i32()? as i64;
            let text = rd.string()?;
            let sx = rd.i32()? as i64;
            let sy = rd.i32()? as i64;
            let glyphs = rd.extend_items(Self::read_early_mwnd_glyph)?;
            let color = glyphs
                .first()
                .map(|glyph| glyph.moji_color_no)
                .unwrap_or(m.default_moji_color);
            choices.push(runtime::globals::MwndSelectionChoice {
                text,
                kind,
                color,
                pos: (x, y),
                size: (sx, sy),
                glyphs,
            });
        }
        if !choices.is_empty() || cancel_enable {
            m.selection = Some(runtime::globals::MwndSelectionState {
                choices,
                disp_item_count,
                cursor: 0,
                cancel_enable,
                close_mwnd: false,
                result: 0,
            });
        }
        Ok(())
    }

    pub(super) fn read_early_world(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        world_no: i32,
    ) -> Result<runtime::globals::WorldState> {
        let mut world = runtime::globals::WorldState::new(world_no);
        world.mode = rd.i32()?;
        world.camera_eye_x = Self::read_cpp_int_event_raw(rd)?;
        world.camera_eye_y = Self::read_cpp_int_event_raw(rd)?;
        world.camera_eye_z = Self::read_cpp_int_event_raw(rd)?;
        world.camera_pint_x = Self::read_cpp_int_event_raw(rd)?;
        world.camera_pint_y = Self::read_cpp_int_event_raw(rd)?;
        world.camera_pint_z = Self::read_cpp_int_event_raw(rd)?;
        world.camera_up_x = Self::read_cpp_int_event_raw(rd)?;
        world.camera_up_y = Self::read_cpp_int_event_raw(rd)?;
        world.camera_up_z = Self::read_cpp_int_event_raw(rd)?;
        world.camera_view_angle = rd.i32()?;
        world.mono = rd.i32()?;
        // The old world POD ends with the 20-byte camera-eye XZ event.
        rd.skip(20)?;
        Ok(world)
    }

    pub(super) fn read_early_btn_select(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::BtnSelectRuntimeState> {
        let template_no = rd.i32()? as i64;
        let mut param = [0i64; 28];
        for (index, value) in param.iter_mut().enumerate() {
            *value = if index == 19 { -1 } else { rd.i32()? as i64 };
        }
        let appear_flag = rd.bool()?;
        let processing_flag_0 = rd.bool()?;
        let processing_flag_1 = rd.bool()?;
        let processing_flag_2 = rd.bool()?;
        let cancel_enable = rd.bool()?;
        let capture_flag = rd.bool()?;
        let sel_start_call_scn = rd.string()?;
        let sel_start_call_z_no = rd.i32()? as i64;
        let choices = rd.extend_items(|rd| {
            let item_template_no = rd.i32()? as i64;
            let base_file = rd.string()?;
            let filter_file = rd.string()?;
            let text = rd.string()?;
            let item_type = rd.i32()? as i64;
            let color = rd.i32()? as i64;
            let relative_x = rd.i32()? as i64;
            let relative_y = rd.i32()? as i64;
            let glyphs = rd.extend_items(Self::read_early_mwnd_glyph)?;
            Ok(runtime::globals::BtnSelectChoiceState {
                template_no: item_template_no,
                base_file,
                filter_file,
                text,
                item_type,
                color,
                pos: (
                    param[0].saturating_add(relative_x),
                    param[1].saturating_add(relative_y),
                ),
                size: (0, 0),
                glyphs,
            })
        })?;
        let cursor = choices
            .iter()
            .position(|choice| choice.item_type == 1)
            .unwrap_or(0);

        Ok(runtime::globals::BtnSelectRuntimeState {
            template_no,
            saved_cur_param: Some(param),
            choices,
            cursor,
            pressed_index: None,
            pressed_inside: false,
            cancel_enable,
            capture_flag,
            // load() calls restruct_template() after init_work_variable().  The
            // saved processing flags are restored, but m_sync_type and every
            // animation work member remain zero exactly as in the C++ object.
            started: processing_flag_0,
            result: 0,
            sync_type: 0,
            read_flag_scene_no: -1,
            read_flag_flag_no: -1,
            sel_start_call_scn,
            sel_start_call_z_no,
            appear_flag,
            open_anime_type: 0,
            open_anime_time: 0,
            open_anime_cur_time: 0,
            close_anime_type: 0,
            close_anime_time: 0,
            close_anime_cur_time: 0,
            decide_anime_type: 0,
            decide_anime_time: 0,
            decide_anime_cur_time: 0,
            decide_sel_no: -1,
            processing_flag_0,
            processing_flag_1,
            processing_flag_2,
            capture_now_flag: false,
            result_delivered: false,
        })
    }

    pub(super) fn read_early_stage(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        stage_idx: i64,
    ) -> Result<(
        runtime::globals::StageFormState,
        runtime::globals::BtnSelectRuntimeState,
    )> {
        let mut st = runtime::globals::StageFormState::default();
        st.initialized_from_gameexe = true;
        st.group_lists
            .insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_group(rd))?);
        st.object_lists
            .insert(stage_idx, rd.fixed_items(|rd| Self::read_early_object(rd))?);
        st.mwnd_lists
            .insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_mwnd(rd))?);
        let btn_select = Self::read_early_btn_select(rd)?;
        st.btn_select_states.insert(stage_idx, btn_select.clone());
        st.effect_lists
            .insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_effect(rd))?);
        st.quake_lists
            .insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_quake(rd))?);
        Ok((st, btn_select))
    }

    pub(super) fn read_early_msg_back(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::MsgBackState> {
        let cnt = rd.i32()?.max(0) as usize;
        let mut st = runtime::globals::MsgBackState::default();
        st.history.clear();
        for _ in 0..cnt {
            let mut entry = runtime::globals::MsgBackEntry::default();
            entry.pct_flag = rd.bool()?;
            entry.msg_str = rd.string()?;
            entry.original_name = rd.string()?;
            entry.disp_name = rd.string()?;
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
}
