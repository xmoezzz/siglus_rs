//! eluna is a small Rust crate for Emote/PSB reverse engineering work.
//!
//! The current scope is intentionally narrow:
//! - PSB container parsing with explicit MDF/LZ4 normalization and optional key-based decryption;
//! - resource range extraction;
//! - D3D9-compatible Emote mesh vertex layout and strip generation.

pub mod api;
pub mod emote;
pub mod psb;
pub mod reverse;
pub mod runtime;
pub mod schema;
pub mod sdk;
pub mod shader;
pub mod vertex;

pub use emote::{
    load_emote_static_scene, EmoteCameraRuntimeState, EmoteDrawFrameInfo, EmoteDrawPass,
    EmoteFeedbackRuntimeState, EmoteGroundCorrectionHook, EmoteMeshChainNode, EmoteMeshPatch,
    EmoteModelRuntimeState, EmoteModelSchema, EmoteMotionInfo, EmoteSceneBounds, EmoteSchemaError,
    EmoteStereovisionControl, EmoteStereovisionProfile, EmoteStaticScene,
    EmoteStaticSprite, EmoteStepFrameInput, EmoteStepFrameLayerState, EmoteStepFrameMeshState,
    EmoteStepFrameOutput, EmoteTextureIcon, EmoteTextureSource,
};
pub use psb::{
    adler32, bruteforce_emote_key, normalize_psb_input, PsbBruteforceOptions, PsbBruteforceResult,
    PsbDecryptionKey, PsbError, PsbFile, PsbHeader, PsbNormalizeOptions, PsbResourceRange,
    PsbValue, EMOTE_PSB_KEY0, EMOTE_PSB_KEY1, EMOTE_PSB_KEY2, EMOTE_PSB_KEY4, EMOTE_PSB_KEY5,
};
pub use runtime::{
    collect_emote_runtime_pipeline, collect_emote_timelines, collect_emote_variables,
    BustPhysicsState, ClampControl, ElunaPlayer, EmoteApiLogEntry, EmoteCharaProfileInfo,
    EmoteRuntimePipeline, EmoteStereovisionScreen, EmoteTimeline, EmoteTimelineFrame,
    EmoteTimelineVariable, EmoteVariableFrameInfo, EmoteVariableInfo, EmoteVariableState,
    EmoteVariableTarget, HairPhysicsState, LoopControl, LoopTransition, MirrorControl, OpaqueControl,
    PhysicsControl, PhysicsControlDefinition, SelectorControl, SelectorOption, TransitionControl,
    WindPulse, WindState,
};
pub use schema::{
    collect_resource_refs, collect_schema_paths, top_level_keys, PsbPathEntry, PsbResourceRefs,
    PsbValueKind,
};
pub use sdk::{
    emote_runtime_parity_report, EmoteHideOptions, EmoteLoadOptions, EmoteNewOptions, EmoteRuntime,
    EmoteRuntimeError, EmoteRuntimeParityReport, EmoteShowOptions, EmoteTransOptions,
    EmoteTransformMode,
};
pub use shader::{ScreenUvConstants, ShaderMatrixConstants};
pub use vertex::{
    build_d3d_triangle_strips, expand_triangle_strips_to_list, EmoteVertex, MeshStripBatch,
    VertexBuildError,
};

pub use api::{
    emote_ticks_to_milliseconds, milliseconds_to_emote_ticks, transform_order_mask,
    EmoteDeviceRenderOptions, EmoteMaskMode, EmotePlayerControl, TimelinePlayMode, VariableWrite,
    EMOTE_TICKS_PER_SECOND, EMOTE_UPDATE_MS_CAP,
};
