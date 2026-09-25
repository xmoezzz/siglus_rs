//! Reverse-engineering anchors that are already confirmed from the provided
//! `emotedriver_v.dll` and sqlite decompilation database.
//!
//! This module only records hard facts that are safe to use as implementation
//! constraints. Do not put guessed schema or motion semantics here.

/// Image base used by the inspected `emotedriver_v.dll`.
pub const EMOTEDRIVER_V_IMAGE_BASE: u32 = 0x1000_0000;

/// `MMotionDevice::_RenderMesh`.
///
/// Confirmed behavior in the sqlite pseudocode:
/// `DrawPrimitiveUP(device, 5, primitive_count, vertex_ptr, 24)`.
pub const MMOTION_DEVICE_RENDER_MESH_INNER: u32 = 0x1042_2990;

/// `MMotionDevice::RenderMesh`, the debug wireframe path.
///
/// This path sets FVF `0x42`, disables texture, and draws line strips with
/// primitive type `3`. It is not the textured mesh path.
pub const MMOTION_DEVICE_RENDER_MESH_DEBUG: u32 = 0x1041_8D20;

/// `MMotionDevice::BeforeRender`.
///
/// Confirmed to set FVF `0x142`, render states, texture stage states, sampler
/// states, and when shader mode is enabled, writes vertex shader constants
/// starting at register 0 with four float4 registers.
pub const MMOTION_DEVICE_BEFORE_RENDER: u32 = 0x1041_0580;

/// `MMotionDevice::RestoreD3DState`.
pub const MMOTION_DEVICE_RESTORE_D3D_STATE: u32 = 0x1041_9430;

/// `MMotionPlayer::ParseVariablePath`.
pub const MMOTION_PLAYER_PARSE_VARIABLE_PATH: u32 = 0x1034_8EE0;

/// `MMotionPlayer::FindParameter`.
pub const MMOTION_PLAYER_FIND_PARAMETER: u32 = 0x1034_2350;

/// `MMotionPlayer::GetShapeParam`.
pub const MMOTION_PLAYER_GET_SHAPE_PARAM: u32 = 0x1034_6540;

/// Loader/parser path that reads `parameter`.
pub const MMOTION_PLAYER_LOAD_PARAMETER: u32 = 0x1034_9650;

/// Loader/parser path that reads `parameterize`.
pub const MMOTION_PLAYER_LOAD_PARAMETERIZE: u32 = 0x1030_DC80;

/// D3D9 vtable byte offset for `IDirect3DDevice9::SetRenderState`.
pub const D3D9_VT_SET_RENDER_STATE: usize = 224;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetTexture`.
pub const D3D9_VT_SET_TEXTURE: usize = 260;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetTextureStageState`.
pub const D3D9_VT_SET_TEXTURE_STAGE_STATE: usize = 268;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetSamplerState`.
pub const D3D9_VT_SET_SAMPLER_STATE: usize = 276;
/// D3D9 vtable byte offset for `IDirect3DDevice9::DrawPrimitiveUP`.
pub const D3D9_VT_DRAW_PRIMITIVE_UP: usize = 332;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetFVF`.
pub const D3D9_VT_SET_FVF: usize = 356;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetVertexShader`.
pub const D3D9_VT_SET_VERTEX_SHADER: usize = 368;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetVertexShaderConstantF`.
pub const D3D9_VT_SET_VERTEX_SHADER_CONSTANT_F: usize = 376;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetPixelShader`.
pub const D3D9_VT_SET_PIXEL_SHADER: usize = 388;
/// D3D9 vtable byte offset for `IDirect3DDevice9::SetPixelShaderConstantF`.
pub const D3D9_VT_SET_PIXEL_SHADER_CONSTANT_F: usize = 396;

/// D3D9 primitive value used by the real textured mesh path.
pub const D3DPT_TRIANGLESTRIP_VALUE: u32 = 5;

/// D3D9 primitive value used by the debug wireframe path.
///
/// This is `D3DPT_LINESTRIP`, not triangle strip.
pub const D3DPT_LINESTRIP_VALUE: u32 = 3;

/// Textured mesh FVF set by `BeforeRender` and restored after debug wireframe.
pub const D3DFVF_TEXTURED_MESH: u32 = 0x142;

/// Debug wireframe FVF set by `MMotionDevice::RenderMesh`.
pub const D3DFVF_DEBUG_WIREFRAME: u32 = 0x42;

/// Number of bytes per textured mesh vertex in `_RenderMesh`.
pub const TEXTURED_MESH_VERTEX_STRIDE: usize = 0x18;

/// Number of bytes per debug wireframe vertex in `RenderMesh`.
///
/// The call still passes stride 24 even though FVF `0x42` only consumes
/// position plus diffuse. The unused tail bytes must not be interpreted as
/// UVs in the debug path.
pub const DEBUG_WIREFRAME_VERTEX_STRIDE: usize = 0x18;
