//! One-time cache of all env switches read across this crate, so hot paths
//! (trace guards, per-frame timing, wait/anim skip checks) pay a single atomic
//! load instead of a `std::env::var` syscall per call. Environment is
//! immutable after process start, so every switch is loaded once and read
//! from memory afterwards.

use std::sync::OnceLock;

/// Value semantics for the "truthy" switches (`matches!` on 1/true/TRUE/yes/YES).
fn env_flag(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

/// Presence semantics (`is_some()`).
fn env_present(key: &str) -> bool {
    std::env::var_os(key).is_some()
}

/// Cached env switches. Environment is immutable after process start, so the
/// values are read once and never change.
pub struct TraceEnv {
    // Shared
    /// `SG_DEBUG` presence (`var_os.is_some()`) — used by vm.rs / stage.rs.
    pub sg_debug: bool,
    /// `SG_DEBUG` as a truthy flag (`matches!` on 1/true/TRUE/yes/YES) —
    /// used by runtime/mod.rs.
    pub sg_debug_value: bool,
    /// `SG_CONFIG_BUTTON_TRACE` truthy flag.
    pub sg_config_button_trace: bool,
    /// `SG_MWND_OBJECT_TRACE` truthy flag.
    pub sg_mwnd_object_trace: bool,

    // runtime/mod.rs
    pub sg_input_trace: bool,
    pub sg_render_tree_debug: bool,
    pub sg_ctx_tick_trace: bool,
    pub sg_msgbk_trace: bool,
    pub sg_movie_trace: bool,
    pub sg_object_motion_trace: bool,
    pub siglus_trace_codes: bool,

    // audio
    pub sg_audio_trace: bool,

    // runtime/forms/counter.rs
    pub sg_counter_trace: bool,

    // vm.rs
    pub tick_trace: bool,
    pub frame_action_trace: bool,
    pub syscom_proc_trace: bool,
    pub title_chain_trace: bool,
    pub call_return_pc_trace: bool,
    pub frame_action_call_trace: bool,
    pub vm_commands_trace: bool,
    pub unknown_forms_trace: bool,
    pub vm_trace: bool,
    pub vm_trace_scene: Option<String>,
    pub vm_trace_pc: Option<(usize, usize)>,
    pub inline_user_cmd_max_steps: u64,
    pub frame_action_max_steps: u64,
    /// `SG_PROC_FLOW_TRACE` presence (vm.rs reads this as `proc_flow_trace`).
    pub proc_flow_trace: bool,
    /// `SG_SAVELOAD_TRACE` presence (vm.rs reads this as `saveload_trace`).
    pub saveload_trace: bool,

    // stage.rs
    pub title_hit_trace: bool,
    pub trace_object_slots: Vec<usize>,
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_pc_range(s: &str) -> Option<(usize, usize)> {
    let parse = |s: &str| {
        usize::from_str_radix(s.trim_start_matches("0x"), 16)
            .or_else(|_| s.parse::<usize>())
    };
    s.split_once("..")
        .and_then(|(start, end)| Some((parse(start).ok()?, parse(end).ok()?)))
}

impl TraceEnv {
    pub fn load() -> Self {
        Self {
            sg_debug: env_present("SG_DEBUG"),
            sg_debug_value: env_flag("SG_DEBUG"),
            sg_config_button_trace: env_flag("SG_CONFIG_BUTTON_TRACE"),
            sg_mwnd_object_trace: env_flag("SG_MWND_OBJECT_TRACE"),
            sg_input_trace: env_flag("SG_INPUT_TRACE"),
            sg_render_tree_debug: env_flag("SG_RENDER_TREE_DEBUG"),
            sg_ctx_tick_trace: env_present("SG_CTX_TICK_TRACE"),
            sg_msgbk_trace: env_present("SG_MSGBK_TRACE"),
            sg_movie_trace: env_present("SG_MOVIE_TRACE"),
            sg_object_motion_trace: env_present("SG_OBJECT_MOTION_TRACE"),
            siglus_trace_codes: env_present("SIGLUS_TRACE_CODES"),
            sg_audio_trace: env_present("SG_AUDIO_TRACE"),
            sg_counter_trace: env_present("SG_COUNTER_TRACE"),
            tick_trace: env_present("SG_TICK_TRACE"),
            frame_action_trace: env_present("SG_FRAME_ACTION_TRACE"),
            syscom_proc_trace: env_present("SG_SYSCOM_PROC_TRACE"),
            title_chain_trace: env_present("SG_TITLE_CHAIN_TRACE"),
            call_return_pc_trace: env_present("SIGLUS_TRACE_CALL_RETURN_PC"),
            frame_action_call_trace: env_present("SIGLUS_TRACE_FRAME_ACTION_CALL"),
            vm_commands_trace: env_present("SIGLUS_TRACE_VM_COMMANDS"),
            unknown_forms_trace: env_present("SIGLUS_TRACE_UNKNOWN_FORMS"),
            vm_trace: env_present("SIGLUS_TRACE_VM"),
            vm_trace_scene: std::env::var("SIGLUS_TRACE_VM_SCENE")
                .ok()
                .filter(|s| !s.is_empty()),
            vm_trace_pc: std::env::var("SIGLUS_TRACE_VM_PC")
                .ok()
                .and_then(|range| parse_pc_range(&range)),
            inline_user_cmd_max_steps: env_u64("SIGLUS_INLINE_USER_CMD_MAX_STEPS", 0),
            frame_action_max_steps: env_u64("SIGLUS_FRAME_ACTION_MAX_STEPS", 0),
            proc_flow_trace: env_present("SG_PROC_FLOW_TRACE"),
            saveload_trace: env_present("SG_SAVELOAD_TRACE"),
            title_hit_trace: env_present("SG_TITLE_HIT_TRACE"),
            trace_object_slots: std::env::var_os("SG_TRACE_OBJECT_SLOT")
                .map(|raw| {
                    raw.to_string_lossy()
                        .split(',')
                        .filter_map(|s| s.trim().parse::<usize>().ok())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        }
    }
}

/// Cached env accessor used by other modules of this crate. Each module reads
/// the properties it needs directly from the returned struct; the per-module
/// cache structs and the module-level `trace_env()` shims are gone.
pub fn trace_env() -> &'static TraceEnv {
    static TRACE_ENV: OnceLock<TraceEnv> = OnceLock::new();
    TRACE_ENV.get_or_init(TraceEnv::load)
}
