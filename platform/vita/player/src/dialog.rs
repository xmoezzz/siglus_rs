//! Native Vita message dialogs, independent of the game renderer's lifetime.
use std::ffi::CString;
use std::mem::{size_of, zeroed};

use vitasdk_sys::*;

unsafe extern "C" {
    fn vita2d_init() -> i32;
    fn vita2d_fini() -> i32;
    fn vita2d_start_drawing();
    fn vita2d_clear_screen();
    fn vita2d_end_drawing();
    fn vita2d_common_dialog_update() -> i32;
    fn vita2d_swap_buffers();
    fn vita2d_wait_rendering_done();
    fn vita2d_pool_reset();
    fn vita2d_set_vblank_wait(enable: i32);
}

struct Display;
impl Drop for Display {
    fn drop(&mut self) {
        unsafe {
            vita2d_wait_rendering_done();
            vita2d_fini();
        }
    }
}

/// Returns false for Cancel. The native dialog owns confirmation input.
pub(super) fn show(message: &str, allow_cancel: bool) -> Result<bool, String> {
    let mut message = message.replace('\0', " ");
    // SCE_MSG_DIALOG_USER_MSG_SIZE includes the terminating NUL.
    if message.len() >= SCE_MSG_DIALOG_USER_MSG_SIZE as usize {
        let mut end = SCE_MSG_DIALOG_USER_MSG_SIZE as usize - 4;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str("...");
    }
    let message = CString::new(message).map_err(|error| error.to_string())?;
    let rc = unsafe { vita2d_init() };
    if rc < 0 {
        return Err(format!("dialog display init: {rc:#x}"));
    }
    let _display = Display;
    unsafe {
        vita2d_set_vblank_wait(1);
    }
    let mut config: SceCommonDialogConfigParam = unsafe { zeroed() };
    config.sdkVersion = PSP2_SDK_VERSION;
    config.language = SCE_SYSTEM_PARAM_LANG_MAX_VALUE;
    config.enterButtonAssign = SCE_SYSTEM_PARAM_ENTER_BUTTON_MAX_VALUE;
    let rc = unsafe { sceCommonDialogSetConfigParam(&config) };
    if rc < 0 {
        return Err(format!("dialog configuration: {rc:#x}"));
    }
    let mut user: SceMsgDialogUserMessageParam = unsafe { zeroed() };
    user.buttonType = if allow_cancel {
        SCE_MSG_DIALOG_BUTTON_TYPE_OK_CANCEL
    } else {
        SCE_MSG_DIALOG_BUTTON_TYPE_OK
    } as i32;
    user.msg = message.as_ptr();
    let mut param: SceMsgDialogParam = unsafe { zeroed() };
    param.sdkVersion = PSP2_SDK_VERSION;
    param.mode = SCE_MSG_DIALOG_MODE_USER_MSG as i32;
    param.userMsgParam = &mut user;
    // Same initialization as VitaSDK's sceMsgDialogParamInit. Set the magic
    // after the parameter has reached its final address, immediately before use.
    param.commonParam.magic = SCE_COMMON_DIALOG_MAGIC_NUMBER
        .wrapping_add((&param.commonParam as *const SceCommonDialogParam) as u32);
    debug_assert_eq!(size_of::<SceMsgDialogParam>(), 0x88);
    let rc = unsafe { sceMsgDialogInit(&param) };
    if rc < 0 {
        return Err(format!("message dialog init: {rc:#x}"));
    }
    super::vita::log("native message dialog opened");
    let result = loop {
        let status = unsafe { sceMsgDialogGetStatus() };
        if status == SCE_COMMON_DIALOG_STATUS_FINISHED {
            break Ok(());
        }
        if status != SCE_COMMON_DIALOG_STATUS_RUNNING {
            break Err(format!("message dialog stopped: {status}"));
        }
        unsafe {
            vita2d_pool_reset();
            vita2d_start_drawing();
            vita2d_clear_screen();
            vita2d_end_drawing();
        }
        let rc = unsafe { vita2d_common_dialog_update() };
        if rc < 0 {
            break Err(format!("message dialog update: {rc:#x}"));
        }
        unsafe {
            vita2d_swap_buffers();
        }
    };
    let mut response: SceMsgDialogResult = unsafe { zeroed() };
    let rc = if result.is_ok() {
        unsafe { sceMsgDialogGetResult(&mut response) }
    } else {
        unsafe {
            sceMsgDialogAbort();
        }
        -1
    };
    unsafe {
        sceMsgDialogTerm();
    }
    result?;
    if rc < 0 {
        return Err(format!("message dialog result: {rc:#x}"));
    }
    super::vita::log("native message dialog dismissed");
    Ok(response.result == SCE_COMMON_DIALOG_RESULT_OK as i32
        && response.buttonId == SCE_MSG_DIALOG_BUTTON_ID_OK as i32)
}
