use std::error::Error;
use std::fmt;
use std::mem::size_of;
use std::ops::Range;

/// D3D9 FVF observed in `MMotionDevice::_RenderMesh`.
///
/// `0x142 = D3DFVF_XYZ | D3DFVF_DIFFUSE | D3DFVF_TEX1`.
pub const D3DFVF_EMOTE_MESH: u32 = 0x142;

/// D3D9 primitive type observed in `MMotionDevice::_RenderMesh`.
///
/// `5 = D3DPT_TRIANGLESTRIP`.
pub const D3DPT_TRIANGLESTRIP: u32 = 5;

/// Exact 24-byte vertex layout passed to `IDirect3DDevice9::DrawPrimitiveUP`.
///
/// The original call uses stride `0x18`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmoteVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Raw D3DCOLOR value. D3D convention is 0xAARRGGBB.
    pub diffuse_argb: u32,
    pub u: f32,
    pub v: f32,
}

impl EmoteVertex {
    pub const STRIDE: usize = 0x18;

    pub const fn new(x: f32, y: f32, diffuse_argb: u32, u: f32, v: f32) -> Self {
        Self {
            x,
            y,
            z: 0.0,
            diffuse_argb,
            u,
            v,
        }
    }

    /// Converts D3D ARGB color to normalized RGBA for WebGPU-side code.
    pub fn diffuse_rgba_f32(self) -> [f32; 4] {
        let a = ((self.diffuse_argb >> 24) & 0xff) as f32 / 255.0;
        let r = ((self.diffuse_argb >> 16) & 0xff) as f32 / 255.0;
        let g = ((self.diffuse_argb >> 8) & 0xff) as f32 / 255.0;
        let b = (self.diffuse_argb & 0xff) as f32 / 255.0;
        [r, g, b, a]
    }
}

const _: () = assert!(size_of::<EmoteVertex>() == EmoteVertex::STRIDE);

#[derive(Debug, Clone, PartialEq)]
pub struct MeshStripBatch {
    pub vertices: Vec<EmoteVertex>,
    pub strips: Vec<Range<usize>>,
    pub primitive_count_per_strip: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VertexBuildError {
    ZeroDivisionX,
    ZeroDivisionY,
    ZeroTextureWidth,
    ZeroTextureHeight,
    PositionCountMismatch { expected: usize, actual: usize },
    ColorCountMismatch { expected: usize, actual: usize },
    CountOverflow,
}

impl fmt::Display for VertexBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VertexBuildError::ZeroDivisionX => write!(f, "division_x must be greater than zero"),
            VertexBuildError::ZeroDivisionY => write!(f, "division_y must be greater than zero"),
            VertexBuildError::ZeroTextureWidth => {
                write!(f, "texture_width must be greater than zero")
            }
            VertexBuildError::ZeroTextureHeight => {
                write!(f, "texture_height must be greater than zero")
            }
            VertexBuildError::PositionCountMismatch { expected, actual } => {
                write!(
                    f,
                    "position count mismatch: expected {expected}, got {actual}"
                )
            }
            VertexBuildError::ColorCountMismatch { expected, actual } => {
                write!(f, "color count mismatch: expected {expected}, got {actual}")
            }
            VertexBuildError::CountOverflow => write!(f, "mesh vertex count overflow"),
        }
    }
}

impl Error for VertexBuildError {}

/// Builds the same row-by-row triangle strips used by `MMotionDevice::_RenderMesh`.
///
/// Reverse-engineered behavior:
/// - input grid is `(division_x + 1) * (division_y + 1)`;
/// - each row pair is drawn with one `D3DPT_TRIANGLESTRIP` call;
/// - each strip contains `2 * (division_x + 1)` vertices;
/// - D3D primitive count per strip is `2 * division_x`;
/// - vertex stride is `0x18`;
/// - vertex layout is `{ f32 x, f32 y, f32 z=0, u32 diffuse, f32 u, f32 v }`.
pub fn build_d3d_triangle_strips(
    positions_xy: &[[f32; 2]],
    diffuse_argb: &[u32],
    tex_x: f32,
    tex_y: f32,
    tex_w: f32,
    tex_h: f32,
    division_x: usize,
    division_y: usize,
    texture_width: u32,
    texture_height: u32,
) -> Result<MeshStripBatch, VertexBuildError> {
    if division_x == 0 {
        return Err(VertexBuildError::ZeroDivisionX);
    }
    if division_y == 0 {
        return Err(VertexBuildError::ZeroDivisionY);
    }
    if texture_width == 0 {
        return Err(VertexBuildError::ZeroTextureWidth);
    }
    if texture_height == 0 {
        return Err(VertexBuildError::ZeroTextureHeight);
    }

    let cols = division_x
        .checked_add(1)
        .ok_or(VertexBuildError::CountOverflow)?;
    let rows = division_y
        .checked_add(1)
        .ok_or(VertexBuildError::CountOverflow)?;
    let expected = cols
        .checked_mul(rows)
        .ok_or(VertexBuildError::CountOverflow)?;

    if positions_xy.len() != expected {
        return Err(VertexBuildError::PositionCountMismatch {
            expected,
            actual: positions_xy.len(),
        });
    }
    if diffuse_argb.len() != expected {
        return Err(VertexBuildError::ColorCountMismatch {
            expected,
            actual: diffuse_argb.len(),
        });
    }

    let u_step = tex_w / division_x as f32;
    let v_step = tex_h / division_y as f32;
    let u_scale = 1.0 / texture_width as f32;
    let v_scale = 1.0 / texture_height as f32;

    let mut us = Vec::with_capacity(cols);
    for x in 0..cols {
        us.push((tex_x + x as f32 * u_step) * u_scale);
    }

    let mut vs = Vec::with_capacity(rows);
    for y in 0..rows {
        vs.push((tex_y + y as f32 * v_step) * v_scale);
    }

    let strip_vertices = cols.checked_mul(2).ok_or(VertexBuildError::CountOverflow)?;
    let total_vertices = strip_vertices
        .checked_mul(division_y)
        .ok_or(VertexBuildError::CountOverflow)?;

    let mut vertices = Vec::with_capacity(total_vertices);
    let mut strips = Vec::with_capacity(division_y);

    for row in 0..division_y {
        let start = vertices.len();

        for col in 0..cols {
            let top_index = row * cols + col;
            let bottom_index = top_index + cols;

            let top = positions_xy[top_index];
            vertices.push(EmoteVertex::new(
                top[0],
                top[1],
                diffuse_argb[top_index],
                us[col],
                vs[row],
            ));

            let bottom = positions_xy[bottom_index];
            vertices.push(EmoteVertex::new(
                bottom[0],
                bottom[1],
                diffuse_argb[bottom_index],
                us[col],
                vs[row + 1],
            ));
        }

        let end = vertices.len();
        strips.push(start..end);
    }

    Ok(MeshStripBatch {
        vertices,
        strips,
        primitive_count_per_strip: 2 * division_x,
    })
}

/// Expands row-wise triangle strips into a triangle list.
///
/// This is useful for WebGPU backends that do not want to keep strip state.
/// Winding is alternated per strip vertex parity, matching triangle-strip rules.
pub fn expand_triangle_strips_to_list(batch: &MeshStripBatch) -> Vec<EmoteVertex> {
    let mut out = Vec::new();

    for strip in &batch.strips {
        let s = &batch.vertices[strip.clone()];
        if s.len() < 3 {
            continue;
        }

        out.reserve((s.len() - 2) * 3);
        for i in 0..(s.len() - 2) {
            if i & 1 == 0 {
                out.push(s[i]);
                out.push(s[i + 1]);
                out.push(s[i + 2]);
            } else {
                out.push(s[i + 1]);
                out.push(s[i]);
                out.push(s[i + 2]);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_layout_matches_d3d_stride() {
        assert_eq!(size_of::<EmoteVertex>(), 0x18);
        assert_eq!(D3DFVF_EMOTE_MESH, 0x142);
        assert_eq!(D3DPT_TRIANGLESTRIP, 5);
    }

    #[test]
    fn builds_single_cell_strip() {
        let positions = [[0.0, 0.0], [10.0, 0.0], [0.0, 20.0], [10.0, 20.0]];
        let colors = [0xffff0000, 0xff00ff00, 0xff0000ff, 0xffffffff];
        let batch =
            build_d3d_triangle_strips(&positions, &colors, 0.0, 0.0, 10.0, 20.0, 1, 1, 100, 200)
                .unwrap();

        assert_eq!(batch.primitive_count_per_strip, 2);
        assert_eq!(batch.strips, vec![0..4]);
        assert_eq!(batch.vertices.len(), 4);
        assert_eq!(
            batch.vertices[0],
            EmoteVertex::new(0.0, 0.0, 0xffff0000, 0.0, 0.0)
        );
        assert_eq!(
            batch.vertices[1],
            EmoteVertex::new(0.0, 20.0, 0xff0000ff, 0.0, 0.1)
        );
        assert_eq!(
            batch.vertices[2],
            EmoteVertex::new(10.0, 0.0, 0xff00ff00, 0.1, 0.0)
        );
        assert_eq!(
            batch.vertices[3],
            EmoteVertex::new(10.0, 20.0, 0xffffffff, 0.1, 0.1)
        );
    }

    #[test]
    fn expands_strip_to_triangle_list() {
        let positions = [[0.0, 0.0], [10.0, 0.0], [0.0, 20.0], [10.0, 20.0]];
        let colors = [0xffffffff; 4];
        let batch =
            build_d3d_triangle_strips(&positions, &colors, 0.0, 0.0, 10.0, 20.0, 1, 1, 10, 20)
                .unwrap();
        let list = expand_triangle_strips_to_list(&batch);
        assert_eq!(list.len(), 6);
    }
}
