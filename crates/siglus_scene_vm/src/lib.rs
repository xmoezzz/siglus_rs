//! Siglus BG stage: g00 decoding + Siglus-like resource lookup + wgpu rendering.
//!
//! Code comments are intentionally in English.

pub mod app_path;
pub mod assets;
pub mod audio;
pub mod image_manager;
pub mod layer;
pub mod mesh3d;
pub mod movie;
pub mod render_math;
pub mod resource;
pub mod runtime;
pub mod soft_render;
pub mod text_render;

pub mod elm_code;

pub mod scene_stream;
pub mod vm;

// Re-export the format-first asset crate so higher layers (VM/app) can share
// parsers/decoders without wiring a second direct dependency.
pub use siglus_assets as formats;

pub mod render;

pub mod input;

pub mod host;
#[cfg(target_os = "android")]
pub mod android_host;
#[cfg(target_os = "ios")]
pub mod ios_host;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub mod pump_host;

pub mod display_ffi;
