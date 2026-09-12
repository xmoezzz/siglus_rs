//! Version-specific save layouts. Dispatch once, then read fields in disk order.
use anyhow::{bail, ensure, Result};
use std::collections::HashMap;

use crate::original_save::{
    OriginalConfigSaveHeader, OriginalGlobalSaveHeader, OriginalStreamReader,
};
use crate::runtime::globals::OriginalConfigRuntimeState;

pub(super) fn read_config(
    header: OriginalConfigSaveHeader,
    payload: &[u8],
    cfg: &mut OriginalConfigRuntimeState,
    game_id: &str,
) -> Result<()> {
    let mut rd = OriginalStreamReader::new(payload);
    match (header.major_version, header.minor_version) {
        (1, 0) => read_config_v1_0(&mut rd, cfg),
        (1, 1) => read_config_v1_1(&mut rd, cfg),
        (1, 2) => read_config_v1_2(&mut rd, cfg, game_id),
        (1, 3) => read_config_v1_3(&mut rd, cfg),
        (major, minor) => bail!("unsupported config save version {major}.{minor}"),
    }?;
    ensure!(
        rd.remaining().is_empty(),
        "unexpected trailing config save data: {} bytes",
        rd.remaining().len()
    );
    Ok(())
}

fn read_config_v1_0(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    read_screen_v1_0(rd, cfg)?;
    // Five audio categories: BGM, voice, PCM, SE, movie.
    read_config_common(rd, cfg, 5)?;
    read_config_flags(rd, cfg)?;
    // Native 1.0 has an obsolete bool/i32 pair before the save/load toggles.
    rd.bool()?;
    rd.i32()?;
    read_config_paths(rd, cfg)
}

fn read_config_v1_1(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    read_screen_v1_0(rd, cfg)?;
    read_config_common(rd, cfg, 32)?;
    read_config_flags(rd, cfg)?;
    read_config_paths_with_voice(rd, cfg)
}

fn read_config_v1_2(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
    game_id: &str,
) -> Result<()> {
    // The native loader documents this title's 1.3 screen layout under a 1.2 header.
    if game_id == "planetarian [HD Edition]" {
        read_screen_v1_3(rd, cfg)?;
    } else {
        read_screen_v1_0(rd, cfg)?;
    }
    read_config_common(rd, cfg, 32)?;
    read_cursor_autohide(rd, cfg)?;
    read_config_flags(rd, cfg)?;
    read_config_paths_with_voice(rd, cfg)
}

fn read_config_v1_3(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    read_screen_v1_3(rd, cfg)?;
    read_config_common(rd, cfg, 32)?;
    read_cursor_autohide(rd, cfg)?;
    read_config_flags(rd, cfg)?;
    read_config_paths_with_voice(rd, cfg)
}

fn read_screen_v1_0(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    cfg.screen_size_mode = rd.i32()? as i64;
    cfg.screen_size_scale = (rd.i32()? as i64, rd.i32()? as i64);
    Ok(())
}

fn read_screen_v1_3(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    cfg.screen_size_mode = rd.i32()? as i64;
    cfg.screen_size_mode_window = rd.i32()? as i64;
    cfg.screen_size_scale = (rd.i32()? as i64, rd.i32()? as i64);
    cfg.screen_size_free = (rd.i32()? as i64, rd.i32()? as i64);
    Ok(())
}

fn read_cursor_autohide(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    cfg.mouse_cursor_hide_onoff = rd.bool()?;
    cfg.mouse_cursor_hide_time = rd.i32()? as i64;
    Ok(())
}

fn read_config_paths(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    cfg.saveload_alert_flag = rd.bool()?;
    cfg.saveload_dblclick_flag = rd.bool()?;
    cfg.ss_path = rd.string()?;
    cfg.editor_path = rd.string()?;
    Ok(())
}

fn read_config_paths_with_voice(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    read_config_paths(rd, cfg)?;
    cfg.koe_path = rd.string()?;
    cfg.koe_tool_path = rd.string()?;
    Ok(())
}

fn read_config_common(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
    sound_count: usize,
) -> Result<()> {
    cfg.fullscreen_change_resolution = rd.bool()?;
    cfg.fullscreen_display_cnt = rd.i32()? as i64;
    cfg.fullscreen_display_no = rd.i32()? as i64;
    cfg.fullscreen_resolution_cnt = rd.i32()? as i64;
    cfg.fullscreen_resolution_no = rd.i32()? as i64;
    cfg.fullscreen_resolution = (rd.i32()? as i64, rd.i32()? as i64);
    cfg.fullscreen_mode = rd.i32()? as i64;
    cfg.fullscreen_scale = (rd.i32()? as i64, rd.i32()? as i64);
    cfg.fullscreen_scale_sync_switch = rd.bool()?;
    cfg.fullscreen_move = (rd.i32()? as i64, rd.i32()? as i64);
    cfg.all_sound_user_volume = rd.i32()? as i64;
    for value in cfg.sound_user_volume.iter_mut().take(sound_count) {
        *value = rd.i32()? as i64;
    }
    cfg.play_all_sound_check = rd.bool()?;
    for value in cfg.play_sound_check.iter_mut().take(sound_count) {
        *value = rd.bool()?;
    }
    cfg.bgmfade_volume = rd.i32()? as i64;
    cfg.bgmfade_use_check = rd.bool()?;
    cfg.filter_color_argb = u32::from_le_bytes(rd.take_raw(4)?.try_into().unwrap());
    cfg.font_proportional = rd.bool()?;
    cfg.font_name = rd.string()?;
    cfg.font_shadow = rd.i32()? as i64;
    cfg.font_futoku = rd.bool()?;
    cfg.message_speed = rd.i32()? as i64;
    cfg.message_speed_nowait = rd.bool()?;
    cfg.auto_mode_onoff = rd.bool()?;
    cfg.auto_mode_moji_wait = rd.i32()? as i64;
    cfg.auto_mode_min_wait = rd.i32()? as i64;
    Ok(())
}

fn read_config_flags(
    rd: &mut OriginalStreamReader<'_>,
    cfg: &mut OriginalConfigRuntimeState,
) -> Result<()> {
    cfg.jitan_normal_onoff = rd.bool()?;
    cfg.jitan_auto_mode_onoff = rd.bool()?;
    cfg.jitan_msgbk_onoff = rd.bool()?;
    cfg.jitan_speed = rd.i32()? as i64;
    cfg.koe_mode = rd.i32()? as i64;
    let chrkoe_count_raw = rd.i32()?;
    anyhow::ensure!(
        (0..=256).contains(&chrkoe_count_raw),
        "invalid config.sav CHRKOE count: {chrkoe_count_raw}"
    );
    let chrkoe_count = chrkoe_count_raw as usize;
    cfg.chrkoe.clear();
    cfg.chrkoe.reserve(chrkoe_count);
    for _ in 0..chrkoe_count {
        let onoff = rd.bool()?;
        rd.skip(3)?;
        let volume = rd.i32()? as i64;
        cfg.chrkoe.push(crate::runtime::globals::ConfigChrKoeState {
            onoff,
            volume: volume.clamp(0, 255),
        });
    }
    cfg.message_chrcolor_flag = rd.bool()?;
    let object_count_raw = rd.i32()?;
    anyhow::ensure!(
        object_count_raw == 4,
        "invalid config.sav OBJECT_DISP count: {object_count_raw}"
    );
    let object_count = object_count_raw as usize;
    cfg.object_disp_flag.clear();
    for _ in 0..object_count {
        cfg.object_disp_flag.push(rd.bool()?);
    }
    let switch_count_raw = rd.i32()?;
    anyhow::ensure!(
        switch_count_raw == 4,
        "invalid config.sav GLOBAL_EXTRA_SWITCH count: {switch_count_raw}"
    );
    let switch_count = switch_count_raw as usize;
    cfg.global_extra_switch_flag.clear();
    for _ in 0..switch_count {
        cfg.global_extra_switch_flag.push(rd.bool()?);
    }
    let mode_count_raw = rd.i32()?;
    anyhow::ensure!(
        mode_count_raw == 4,
        "invalid config.sav GLOBAL_EXTRA_MODE count: {mode_count_raw}"
    );
    let mode_count = mode_count_raw as usize;
    cfg.global_extra_mode_flag.clear();
    for _ in 0..mode_count {
        cfg.global_extra_mode_flag.push(rd.i32()? as i64);
    }
    cfg.sleep_flag = rd.bool()?;
    cfg.no_wipe_anime_flag = rd.bool()?;
    cfg.skip_wipe_anime_flag = rd.bool()?;
    cfg.no_mwnd_anime_flag = rd.bool()?;
    cfg.wheel_next_message_flag = rd.bool()?;
    cfg.koe_dont_stop_flag = rd.bool()?;
    cfg.skip_unread_message_flag = rd.bool()?;
    Ok(())
}

pub(super) struct GlobalSaveData {
    pub total_play_time: i64,
    pub g: Vec<i64>,
    pub z: Vec<i64>,
    pub m: Vec<String>,
    pub namae_global: Vec<String>,
    pub cg: Vec<i64>,
    pub bgm: Vec<i64>,
    pub chrkoe_look_flags: HashMap<String, bool>,
}

pub(super) fn read_global(
    header: OriginalGlobalSaveHeader,
    payload: &[u8],
) -> Result<GlobalSaveData> {
    let mut rd = OriginalStreamReader::new(payload);
    match (header.major_version, header.minor_version) {
        (1, 2) => read_global_v1_2(&mut rd),
        (1, 5) => read_global_v1_5(&mut rd),
        (2, 0) => read_global_v2_0(&mut rd),
        (major, minor) => bail!("unsupported global save version {major}.{minor}"),
    }
}

fn read_global_v1_2(rd: &mut OriginalStreamReader<'_>) -> Result<GlobalSaveData> {
    // Hatsuyuki Sakura: verified against the native 1.2 save.
    read_global_common(rd)
}

fn read_global_v1_5(rd: &mut OriginalStreamReader<'_>) -> Result<GlobalSaveData> {
    // Seishoujo: verified against the native 1.5 save, including empty CG data.
    read_global_common(rd)
}

fn read_global_v2_0(rd: &mut OriginalStreamReader<'_>) -> Result<GlobalSaveData> {
    // Layout from C_tnm_eng::load_global in the available native source.
    read_global_common(rd)
}

fn read_global_common(rd: &mut OriginalStreamReader<'_>) -> Result<GlobalSaveData> {
    let total_play_time = rd.i64()?;
    let g = rd.fixed_i32_list()?;
    let z = rd.fixed_i32_list()?;
    let m = rd.fixed_str_list()?;
    let namae_global = rd.fixed_str_list()?;
    let _dummy_check_id = rd.i32()?;
    // An uninitialized native CG table saves an extendable empty list:
    // one zero count, without a fixed-array jump. A fixed-array jump at
    // this stream position cannot be zero. Also accept fixed empty lists
    // written by earlier Rust builds.
    let cg = if rd.remaining().starts_with(&0i32.to_le_bytes()) {
        rd.extend_i32_list()?
    } else {
        rd.fixed_i32_list()?
    };
    let bgm = rd.fixed_i32_list()?;
    let chrkoe_cnt = rd.i32()?;
    anyhow::ensure!(
        (0..=256).contains(&chrkoe_cnt),
        "invalid global.sav CHRKOE count: {chrkoe_cnt}"
    );
    let mut chrkoe_look_flags = std::collections::HashMap::new();
    for _ in 0..chrkoe_cnt {
        let name = rd.string()?;
        // C_tnm_chrkoe::look_flag is bool, and C_tnm_save_stream::load<T>
        // pops sizeof(T) bytes. Reading an i32 here consumes three bytes
        // from the following field and corrupts the remainder of global.sav.
        let look_flag = rd.bool()?;
        chrkoe_look_flags.insert(name, look_flag);
    }
    ensure!(
        rd.remaining().is_empty(),
        "unexpected trailing global save data: {} bytes",
        rd.remaining().len()
    );
    Ok(GlobalSaveData {
        total_play_time,
        g,
        z,
        m,
        namae_global,
        cg,
        bgm,
        chrkoe_look_flags,
    })
}
