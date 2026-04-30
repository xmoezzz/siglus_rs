//! Renderer-neutral, WGPU-friendly conversion for Shion semantic `.x` scenes.
//!
//! This crate deliberately does not depend on `wgpu`.  It produces plain Rust
//! buffers and descriptors that a WGPU application can upload into its own
//! buffers, textures, bind groups, and pipelines.

use std::collections::{BTreeMap, HashMap};

use shion_xfile::semantic::{
    AnimationKeyBlock, AnimationSet, EffectInstance, Frame, Material, Mesh, MeshMaterialList, Scene,
    SkinWeights, TimedFloatKeys,
};
use shion_xfile::{parse_x, Error, Result};

pub type Mat4 = [f32; 16];

pub const IDENTITY_MAT4: Mat4 = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub max_bone_influences_per_vertex: usize,
    pub keep_unreferenced_materials: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            max_bone_influences_per_vertex: 4,
            keep_unreferenced_materials: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RenderAsset {
    pub nodes: Vec<RenderNode>,
    pub meshes: Vec<RenderMesh>,
    pub materials: Vec<RenderMaterial>,
    pub textures: Vec<RenderTexture>,
    pub bones: Vec<RenderBone>,
    pub animations: Vec<RenderAnimationClip>,
    pub diagnostics: Vec<RenderDiagnostic>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderNode {
    pub name: Option<String>,
    pub parent: Option<usize>,
    /// Matrix values are preserved in `.x` order.  Shion does not transpose them.
    pub local_transform: Mat4,
    /// Parent-composed transform using the same matrix order as `local_transform`.
    pub world_transform: Mat4,
    pub mesh_indices: Vec<usize>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub texcoord: [f32; 2],
    pub color: [f32; 4],
    pub bone_indices: [u16; 4],
    pub bone_weights: [f32; 4],
}

impl Default for RenderVertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            normal: [0.0; 3],
            texcoord: [0.0; 2],
            color: [1.0, 1.0, 1.0, 1.0],
            bone_indices: [0; 4],
            bone_weights: [0.0; 4],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RenderMesh {
    pub name: Option<String>,
    pub node_index: Option<usize>,
    pub source_vertex_count: usize,
    pub source_face_count: usize,
    /// Expanded per-face-corner vertices.  This preserves `.x` face-corner normal indexing.
    pub vertices: Vec<RenderVertex>,
    /// Concatenated triangle indices, grouped by `batches`.
    pub indices: Vec<u32>,
    pub batches: Vec<RenderBatch>,
    pub source_skin_weights: Vec<RenderVertexInfluences>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderBatch {
    pub material_slot: Option<usize>,
    pub material_index: Option<usize>,
    pub index_start: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RenderMaterial {
    pub name: Option<String>,
    pub source_reference: Option<String>,
    pub diffuse_rgba: [f32; 4],
    pub specular_power: f32,
    pub specular_rgb: [f32; 3],
    pub emissive_rgb: [f32; 3],
    pub texture_indices: Vec<usize>,
    pub effect_filenames: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderTexture {
    pub path: String,
}

#[derive(Debug, Clone, Default)]
pub struct RenderBone {
    pub name: String,
    pub node_index: Option<usize>,
    pub offset_matrix: Mat4,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RenderVertexInfluence {
    pub bone_index: usize,
    pub weight: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderVertexInfluences {
    pub source_vertex: usize,
    pub influences: Vec<RenderVertexInfluence>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderAnimationClip {
    pub name: Option<String>,
    pub ticks_per_second: Option<u32>,
    pub tracks: Vec<RenderAnimationTrack>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderAnimationTrack {
    pub name: Option<String>,
    pub target_name: Option<String>,
    pub target_node: Option<usize>,
    pub key_blocks: Vec<RenderAnimationKeyBlock>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderAnimationKeyBlock {
    pub source_key_type: u32,
    pub keys: Vec<RenderAnimationKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderAnimationKey {
    pub time: u32,
    pub value: RenderAnimationValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenderAnimationValue {
    RotationQuaternion([f32; 4]),
    Scale([f32; 3]),
    Translation([f32; 3]),
    Matrix { key_type: u32, matrix: Mat4 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderDiagnostic {
    pub path: String,
    pub message: String,
}

pub fn render_asset_from_bytes(bytes: &[u8]) -> Result<RenderAsset> {
    render_asset_from_bytes_with_options(bytes, &RenderOptions::default())
}

pub fn render_asset_from_bytes_with_options(bytes: &[u8], options: &RenderOptions) -> Result<RenderAsset> {
    let file = parse_x(bytes)?;
    let scene = Scene::from_xfile(&file)?;
    to_render_asset_with_options(&scene, options)
}

pub fn to_render_asset(scene: &Scene) -> Result<RenderAsset> {
    to_render_asset_with_options(scene, &RenderOptions::default())
}

pub fn to_render_asset_with_options(scene: &Scene, options: &RenderOptions) -> Result<RenderAsset> {
    let mut builder = RenderBuilder::new(scene, options);
    builder.convert_scene()?;
    Ok(builder.asset)
}

struct RenderBuilder<'a> {
    scene: &'a Scene,
    options: &'a RenderOptions,
    asset: RenderAsset,
    texture_lookup: HashMap<String, usize>,
    material_lookup: HashMap<String, usize>,
    node_lookup: HashMap<String, usize>,
    bone_lookup: HashMap<String, usize>,
}

impl<'a> RenderBuilder<'a> {
    fn new(scene: &'a Scene, options: &'a RenderOptions) -> Self {
        Self {
            scene,
            options,
            asset: RenderAsset::default(),
            texture_lookup: HashMap::new(),
            material_lookup: HashMap::new(),
            node_lookup: HashMap::new(),
            bone_lookup: HashMap::new(),
        }
    }

    fn convert_scene(&mut self) -> Result<()> {
        if self.options.keep_unreferenced_materials {
            for material in &self.scene.loose_materials {
                self.add_material(material, None);
            }
        }

        let mut root_nodes = Vec::new();
        for frame in &self.scene.frames {
            root_nodes.push(self.add_frame_nodes(frame, None, IDENTITY_MAT4));
        }

        for (frame, node_index) in self.scene.frames.iter().zip(root_nodes) {
            self.convert_frame_meshes(frame, node_index)?;
        }

        for (mesh_index, mesh) in self.scene.loose_meshes.iter().enumerate() {
            let path = format!("Scene/LooseMesh[{mesh_index}]");
            let render_mesh = self.convert_mesh(mesh, None, &path)?;
            self.asset.meshes.push(render_mesh);
        }

        for animation_set in &self.scene.animation_sets {
            self.asset.animations.push(self.convert_animation_set(animation_set)?);
        }

        Ok(())
    }

    fn add_frame_nodes(&mut self, frame: &Frame, parent: Option<usize>, parent_world: Mat4) -> usize {
        let local = frame.transform.unwrap_or(IDENTITY_MAT4);
        // DirectX .x matrices are kept in their original row-vector order.
        // A child point is transformed as: point * child_local * parent_world.
        let world = mat4_mul(local, parent_world);
        let node_index = self.asset.nodes.len();
        if let Some(name) = &frame.name {
            self.node_lookup.insert(name.clone(), node_index);
        }
        self.asset.nodes.push(RenderNode {
            name: frame.name.clone(),
            parent,
            local_transform: local,
            world_transform: world,
            mesh_indices: Vec::new(),
        });
        for child in &frame.child_frames {
            self.add_frame_nodes(child, Some(node_index), world);
        }
        node_index
    }

    fn convert_frame_meshes(&mut self, frame: &Frame, node_index: usize) -> Result<()> {
        for (mesh_index, mesh) in frame.meshes.iter().enumerate() {
            let path = format!("Frame({})/Mesh[{mesh_index}]", frame.name.as_deref().unwrap_or("<unnamed>"));
            let render_mesh = self.convert_mesh(mesh, Some(node_index), &path)?;
            let render_mesh_index = self.asset.meshes.len();
            self.asset.meshes.push(render_mesh);
            self.asset.nodes[node_index].mesh_indices.push(render_mesh_index);
        }
        let first_child_node = node_index + 1;
        let mut next_child_node = first_child_node;
        for child in &frame.child_frames {
            let child_node = next_child_node;
            self.convert_frame_meshes(child, child_node)?;
            next_child_node += count_frame_tree_nodes(child);
        }
        Ok(())
    }

    fn convert_mesh(&mut self, mesh: &Mesh, node_index: Option<usize>, path: &str) -> Result<RenderMesh> {
        let material_slots = self.resolve_mesh_materials(mesh.material_list.as_ref(), path)?;
        let source_influences = self.collect_vertex_influences(mesh, path)?;
        let color_map = build_vertex_color_map(mesh);
        let mut batch_indices: BTreeMap<Option<usize>, Vec<u32>> = BTreeMap::new();
        let mut vertices = Vec::new();

        for (face_index, face) in mesh.faces.iter().enumerate() {
            if face.len() < 3 {
                return Err(Error::Semantic(format!("{path}: face {face_index} has fewer than 3 vertices")));
            }
            let material_slot = material_slot_for_face(mesh.material_list.as_ref(), face_index)?;
            let material_index = match material_slot {
                Some(slot) => Some(*material_slots.get(slot).ok_or_else(|| {
                    Error::Semantic(format!("{path}: face {face_index} references missing material slot {slot}"))
                })?),
                None => None,
            };

            let mut face_render_indices = Vec::with_capacity(face.len());
            for (corner_index, vertex_index) in face.iter().copied().enumerate() {
                let source_vertex = usize::try_from(vertex_index).map_err(|_| {
                    Error::Semantic(format!("{path}: vertex index overflow in face {face_index}"))
                })?;
                let position = *mesh.vertices.get(source_vertex).ok_or_else(|| {
                    Error::Semantic(format!("{path}: face {face_index} references vertex {source_vertex}, but mesh has {} vertices", mesh.vertices.len()))
                })?;
                let normal = self.normal_for_corner(mesh, face_index, corner_index, path)?;
                let texcoord = match &mesh.texcoords {
                    Some(texcoords) => *texcoords.get(source_vertex).ok_or_else(|| {
                        Error::Semantic(format!("{path}: texcoord missing for source vertex {source_vertex}"))
                    })?,
                    None => {
                        self.warn_once(format!("{path}/MeshTextureCoords"), "mesh has no texture coordinates; emitting [0, 0] texcoords");
                        [0.0, 0.0]
                    }
                };
                let color = color_map.get(&source_vertex).copied().unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let (bone_indices, bone_weights) = pack_influences(
                    source_influences.get(source_vertex).map(|v| v.as_slice()).unwrap_or(&[]),
                    self.options.max_bone_influences_per_vertex,
                    path,
                    source_vertex,
                )?;
                let render_vertex = RenderVertex {
                    position,
                    normal,
                    texcoord,
                    color,
                    bone_indices,
                    bone_weights,
                };
                let render_index = u32::try_from(vertices.len()).map_err(|_| {
                    Error::Semantic(format!("{path}: rendered vertex count exceeds u32::MAX"))
                })?;
                vertices.push(render_vertex);
                face_render_indices.push(render_index);
            }

            let indices = batch_indices.entry(material_index).or_default();
            for i in 1..(face_render_indices.len() - 1) {
                indices.push(face_render_indices[0]);
                indices.push(face_render_indices[i]);
                indices.push(face_render_indices[i + 1]);
            }
        }

        let mut indices = Vec::new();
        let mut batches = Vec::new();
        for (material_index, mut batch) in batch_indices {
            let index_start = u32::try_from(indices.len()).map_err(|_| {
                Error::Semantic(format!("{path}: index buffer offset exceeds u32::MAX"))
            })?;
            let index_count = u32::try_from(batch.len()).map_err(|_| {
                Error::Semantic(format!("{path}: batch index count exceeds u32::MAX"))
            })?;
            let material_slot = material_index.and_then(|global| material_slots.iter().position(|m| *m == global));
            indices.append(&mut batch);
            batches.push(RenderBatch {
                material_slot,
                material_index,
                index_start,
                index_count,
            });
        }

        let source_skin_weights = source_influences
            .into_iter()
            .enumerate()
            .filter(|(_, influences)| !influences.is_empty())
            .map(|(source_vertex, influences)| RenderVertexInfluences { source_vertex, influences })
            .collect();

        Ok(RenderMesh {
            name: mesh.name.clone(),
            node_index,
            source_vertex_count: mesh.vertices.len(),
            source_face_count: mesh.faces.len(),
            vertices,
            indices,
            batches,
            source_skin_weights,
        })
    }

    fn normal_for_corner(&mut self, mesh: &Mesh, face_index: usize, corner_index: usize, path: &str) -> Result<[f32; 3]> {
        let Some(normals) = &mesh.normals else {
            self.warn_once(format!("{path}/MeshNormals"), "mesh has no normals; emitting zero normals");
            return Ok([0.0, 0.0, 0.0]);
        };
        let face_normals = normals.face_normals.get(face_index).ok_or_else(|| {
            Error::Semantic(format!("{path}: MeshNormals is missing face-normal entry for face {face_index}"))
        })?;
        let normal_index = *face_normals.get(corner_index).ok_or_else(|| {
            Error::Semantic(format!("{path}: MeshNormals face {face_index} is missing corner {corner_index}"))
        })? as usize;
        normals.normals.get(normal_index).copied().ok_or_else(|| {
            Error::Semantic(format!("{path}: MeshNormals references normal {normal_index}, but only {} normals exist", normals.normals.len()))
        })
    }

    fn resolve_mesh_materials(&mut self, material_list: Option<&MeshMaterialList>, path: &str) -> Result<Vec<usize>> {
        let Some(material_list) = material_list else {
            return Ok(Vec::new());
        };
        let mut slots = Vec::new();
        for material in &material_list.materials {
            slots.push(self.add_material(material, None));
        }
        for reference in &material_list.material_references {
            let Some(name) = &reference.name else {
                return Err(Error::Semantic(format!("{path}: material reference without name cannot be made render-ready")));
            };
            if let Some(index) = self.material_lookup.get(name).copied() {
                slots.push(index);
            } else {
                let index = self.asset.materials.len();
                self.asset.materials.push(RenderMaterial {
                    name: Some(name.clone()),
                    source_reference: Some(name.clone()),
                    diffuse_rgba: [1.0, 1.0, 1.0, 1.0],
                    specular_power: 0.0,
                    specular_rgb: [0.0, 0.0, 0.0],
                    emissive_rgb: [0.0, 0.0, 0.0],
                    texture_indices: Vec::new(),
                    effect_filenames: Vec::new(),
                });
                self.material_lookup.insert(name.clone(), index);
                self.asset.diagnostics.push(RenderDiagnostic {
                    path: path.to_string(),
                    message: format!("unresolved material reference {name}; emitted placeholder material"),
                });
                slots.push(index);
            }
        }
        Ok(slots)
    }

    fn add_material(&mut self, material: &Material, reference_name: Option<String>) -> usize {
        if let Some(name) = &material.name {
            if let Some(index) = self.material_lookup.get(name).copied() {
                return index;
            }
        }
        let texture_indices = material
            .texture_filenames
            .iter()
            .map(|path| self.add_texture(path))
            .collect();
        let effect_filenames = material
            .effect_instances
            .iter()
            .map(effect_filename)
            .collect();
        let index = self.asset.materials.len();
        self.asset.materials.push(RenderMaterial {
            name: material.name.clone(),
            source_reference: reference_name,
            diffuse_rgba: material.face_color,
            specular_power: material.power,
            specular_rgb: material.specular_color,
            emissive_rgb: material.emissive_color,
            texture_indices,
            effect_filenames,
        });
        if let Some(name) = &material.name {
            self.material_lookup.insert(name.clone(), index);
        }
        index
    }

    fn add_texture(&mut self, path: &str) -> usize {
        if let Some(index) = self.texture_lookup.get(path).copied() {
            return index;
        }
        let index = self.asset.textures.len();
        self.asset.textures.push(RenderTexture { path: path.to_string() });
        self.texture_lookup.insert(path.to_string(), index);
        index
    }

    fn collect_vertex_influences(&mut self, mesh: &Mesh, path: &str) -> Result<Vec<Vec<RenderVertexInfluence>>> {
        let mut influences = vec![Vec::new(); mesh.vertices.len()];
        for skin in &mesh.skin_weights {
            let bone_index = self.add_bone(skin);
            for (i, vertex_index) in skin.vertex_indices.iter().copied().enumerate() {
                let source_vertex = usize::try_from(vertex_index).map_err(|_| {
                    Error::Semantic(format!("{path}: skin vertex index overflow"))
                })?;
                let weight = *skin.weights.get(i).ok_or_else(|| {
                    Error::Semantic(format!("{path}: SkinWeights index/weight length mismatch for bone {}", skin.transform_node_name))
                })?;
                let slot = influences.get_mut(source_vertex).ok_or_else(|| {
                    Error::Semantic(format!("{path}: SkinWeights for bone {} references vertex {source_vertex}, but mesh has {} vertices", skin.transform_node_name, mesh.vertices.len()))
                })?;
                slot.push(RenderVertexInfluence { bone_index, weight });
            }
        }
        Ok(influences)
    }

    fn add_bone(&mut self, skin: &SkinWeights) -> usize {
        if let Some(index) = self.bone_lookup.get(&skin.transform_node_name).copied() {
            return index;
        }
        let node_index = self.node_lookup.get(&skin.transform_node_name).copied();
        let index = self.asset.bones.len();
        self.asset.bones.push(RenderBone {
            name: skin.transform_node_name.clone(),
            node_index,
            offset_matrix: skin.matrix_offset,
        });
        self.bone_lookup.insert(skin.transform_node_name.clone(), index);
        index
    }

    fn convert_animation_set(&self, animation_set: &AnimationSet) -> Result<RenderAnimationClip> {
        let mut clip = RenderAnimationClip {
            name: animation_set.name.clone(),
            ticks_per_second: self.scene.anim_ticks_per_second,
            tracks: Vec::new(),
        };
        for animation in &animation_set.animations {
            let target_node = animation.target_name.as_ref().and_then(|name| self.node_lookup.get(name).copied());
            let mut track = RenderAnimationTrack {
                name: animation.name.clone(),
                target_name: animation.target_name.clone(),
                target_node,
                key_blocks: Vec::new(),
            };
            for block in &animation.keys {
                track.key_blocks.push(convert_animation_key_block(block)?);
            }
            clip.tracks.push(track);
        }
        Ok(clip)
    }

    fn warn_once(&mut self, path: String, message: &str) {
        if !self.asset.diagnostics.iter().any(|d| d.path == path && d.message == message) {
            self.asset.diagnostics.push(RenderDiagnostic { path, message: message.to_string() });
        }
    }
}

fn convert_animation_key_block(block: &AnimationKeyBlock) -> Result<RenderAnimationKeyBlock> {
    let mut keys = Vec::with_capacity(block.keys.len());
    for key in &block.keys {
        keys.push(RenderAnimationKey {
            time: key.time,
            value: convert_animation_key_value(block.key_type, key)?,
        });
    }
    Ok(RenderAnimationKeyBlock {
        source_key_type: block.key_type,
        keys,
    })
}

fn convert_animation_key_value(key_type: u32, key: &TimedFloatKeys) -> Result<RenderAnimationValue> {
    match key_type {
        0 => Ok(RenderAnimationValue::RotationQuaternion(array4(&key.values)?)),
        1 => Ok(RenderAnimationValue::Scale(array3(&key.values)?)),
        2 => Ok(RenderAnimationValue::Translation(array3(&key.values)?)),
        3 | 4 => Ok(RenderAnimationValue::Matrix { key_type, matrix: array16(&key.values)? }),
        other => Err(Error::Semantic(format!("unsupported AnimationKey type {other}"))),
    }
}

fn array3(values: &[f32]) -> Result<[f32; 3]> {
    if values.len() != 3 {
        return Err(Error::Semantic(format!("expected 3 floats, got {}", values.len())));
    }
    Ok([values[0], values[1], values[2]])
}

fn array4(values: &[f32]) -> Result<[f32; 4]> {
    if values.len() != 4 {
        return Err(Error::Semantic(format!("expected 4 floats, got {}", values.len())));
    }
    Ok([values[0], values[1], values[2], values[3]])
}

fn array16(values: &[f32]) -> Result<[f32; 16]> {
    if values.len() != 16 {
        return Err(Error::Semantic(format!("expected 16 floats, got {}", values.len())));
    }
    let mut out = [0.0f32; 16];
    out.copy_from_slice(values);
    Ok(out)
}

fn material_slot_for_face(material_list: Option<&MeshMaterialList>, face_index: usize) -> Result<Option<usize>> {
    let Some(material_list) = material_list else {
        return Ok(None);
    };
    if material_list.materials.is_empty() && material_list.material_references.is_empty() {
        return Ok(None);
    }
    if material_list.face_indexes.is_empty() {
        return Ok(Some(0));
    }
    let slot = *material_list.face_indexes.get(face_index).ok_or_else(|| {
        Error::Semantic(format!("MeshMaterialList is missing material index for face {face_index}"))
    })? as usize;
    Ok(Some(slot))
}

fn count_frame_tree_nodes(frame: &Frame) -> usize {
    1 + frame.child_frames.iter().map(count_frame_tree_nodes).sum::<usize>()
}

fn build_vertex_color_map(mesh: &Mesh) -> HashMap<usize, [f32; 4]> {
    let mut map = HashMap::new();
    if let Some(colors) = &mesh.vertex_colors {
        for color in colors {
            map.insert(color.index as usize, color.rgba);
        }
    }
    map
}

fn pack_influences(
    influences: &[RenderVertexInfluence],
    max: usize,
    path: &str,
    source_vertex: usize,
) -> Result<([u16; 4], [f32; 4])> {
    if max > 4 {
        return Err(Error::Semantic(format!("RenderOptions.max_bone_influences_per_vertex must be <= 4 for the built-in RenderVertex layout, got {max}")));
    }
    if influences.len() > max {
        return Err(Error::Semantic(format!("{path}: source vertex {source_vertex} has {} bone influences, max configured is {max}", influences.len())));
    }
    let mut bone_indices = [0u16; 4];
    let mut bone_weights = [0.0f32; 4];
    for (slot, influence) in influences.iter().enumerate() {
        bone_indices[slot] = u16::try_from(influence.bone_index).map_err(|_| {
            Error::Semantic(format!("{path}: bone index {} does not fit in u16", influence.bone_index))
        })?;
        bone_weights[slot] = influence.weight;
    }
    Ok((bone_indices, bone_weights))
}

fn effect_filename(effect: &EffectInstance) -> String {
    effect.effect_filename.clone()
}

pub fn mat4_mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [0.0f32; 16];
    for row in 0..4 {
        for col in 0..4 {
            out[row * 4 + col] =
                a[row * 4] * b[col]
                    + a[row * 4 + 1] * b[4 + col]
                    + a[row * 4 + 2] * b[8 + col]
                    + a[row * 4 + 3] * b[12 + col];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use shion_xfile::semantic::{Mesh, Scene};

    #[test]
    fn triangulates_quad_with_expanded_vertices() {
        let scene = Scene {
            loose_meshes: vec![Mesh {
                vertices: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0],
                ],
                faces: vec![vec![0, 1, 2, 3]],
                ..Mesh::default()
            }],
            ..Scene::default()
        };
        let asset = to_render_asset(&scene).unwrap();
        assert_eq!(asset.meshes.len(), 1);
        assert_eq!(asset.meshes[0].vertices.len(), 4);
        assert_eq!(asset.meshes[0].indices, vec![0, 1, 2, 0, 2, 3]);
    }
}
