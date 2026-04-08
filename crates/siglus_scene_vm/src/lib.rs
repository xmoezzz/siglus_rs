//! Siglus BG stage: g00 decoding + Siglus-like resource lookup + wgpu rendering.
//!
//! Code comments are intentionally in English.

pub mod assets;
pub mod audio;
pub mod app_path;
pub mod movie;
pub mod image_manager;
pub mod layer;
pub mod resource;
pub mod soft_render;
pub mod text_render;
pub mod runtime;

pub mod elm_code;

pub mod scene_stream;
pub mod vm;

// Re-export the format-first asset crate so higher layers (VM/app) can share
// parsers/decoders without wiring a second direct dependency.
pub use siglus_assets as formats;

#[cfg(feature = "wgpu-winit")]
pub mod render;

#[cfg(feature = "wgpu-winit")]
pub mod input;
