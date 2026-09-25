//! Shader-side math recovered from the embedded D3D9 shader bytecode.
//!
//! These helpers are intentionally small and explicit. They implement the math
//! visible in shader assembly, not a guessed full renderer.

/// Vertex shader constants used by the simple transform path.
///
/// The original driver writes four float4 registers starting at vertex shader
/// constant register 0. The shader then computes:
///
/// ```text
/// pos4 = float4(x, y, z, 1)
/// clip.x = dot(pos4, c0)
/// clip.y = dot(pos4, c1)
/// clip.z = dot(pos4, c2)
/// clip.w = dot(pos4, c3)
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShaderMatrixConstants {
    pub registers: [[f32; 4]; 4],
}

impl ShaderMatrixConstants {
    pub const IDENTITY: Self = Self {
        registers: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    pub fn transform_position(self, position_xyz: [f32; 3]) -> [f32; 4] {
        let p = [position_xyz[0], position_xyz[1], position_xyz[2], 1.0];
        [
            dot4(p, self.registers[0]),
            dot4(p, self.registers[1]),
            dot4(p, self.registers[2]),
            dot4(p, self.registers[3]),
        ]
    }
}

/// Constants equivalent to shader register `c4` in the screen-UV vertex shader
/// variants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenUvConstants {
    pub offset_x: f32,
    pub offset_y: f32,
    pub scale_y: f32,
    pub scale_x: f32,
}

impl ScreenUvConstants {
    pub const D3D_NORMALIZED_VIEWPORT: Self = Self {
        offset_x: 0.5,
        offset_y: 0.5,
        scale_y: 1.0,
        scale_x: 1.0,
    };

    /// Matches the final shader instruction:
    ///
    /// ```text
    /// screen_u =  ndc_x * 0.5 + c4.x
    /// screen_v = -ndc_y * 0.5 + c4.y
    /// ```
    pub fn screen_uv_from_clip(self, clip_xyzw: [f32; 4]) -> Option<[f32; 2]> {
        if clip_xyzw[3] == 0.0 {
            return None;
        }
        let inv_w = 1.0 / clip_xyzw[3];
        let ndc_x = clip_xyzw[0] * inv_w;
        let ndc_y = clip_xyzw[1] * inv_w;
        Some([ndc_x * 0.5 + self.offset_x, ndc_y * -0.5 + self.offset_y])
    }
}

fn dot4(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// WGSL equivalent of the verified simple transform vertex shader path.
pub const WGSL_BASIC_TEXTURED_MESH: &str = r#"
struct MatrixConstants {
    rows: array<vec4<f32>, 4>,
};

@group(0) @binding(0)
var<uniform> matrix_constants: MatrixConstants;

@group(0) @binding(1)
var base_texture: texture_2d<f32>;

@group(0) @binding(2)
var base_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    let p = vec4<f32>(input.position, 1.0);
    var out: VertexOut;
    out.clip_position = vec4<f32>(
        dot(p, matrix_constants.rows[0]),
        dot(p, matrix_constants.rows[1]),
        dot(p, matrix_constants.rows[2]),
        dot(p, matrix_constants.rows[3])
    );
    out.color = input.color;
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(base_texture, base_sampler, input.uv) * input.color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_transform_matches_position() {
        let out = ShaderMatrixConstants::IDENTITY.transform_position([2.0, 3.0, 4.0]);
        assert_eq!(out, [2.0, 3.0, 4.0, 1.0]);
    }

    #[test]
    fn screen_uv_flips_y_like_d3d_shader() {
        let uv = ScreenUvConstants::D3D_NORMALIZED_VIEWPORT
            .screen_uv_from_clip([1.0, 1.0, 0.0, 1.0])
            .unwrap();
        assert_eq!(uv, [1.0, 0.0]);
    }
}
