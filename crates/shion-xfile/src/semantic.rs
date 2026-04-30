use crate::error::{Error, Result};
use crate::model::{PrimitiveValue, ReferenceTarget, XDataObject, XFile, XObjectElement};

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub anim_ticks_per_second: Option<u32>,
    pub loose_booleans: Vec<Boolean>,
    pub loose_boolean2ds: Vec<Boolean2d>,
    pub loose_color_rgbs: Vec<ColorRGB>,
    pub loose_color_rgbas: Vec<ColorRGBA>,
    pub loose_coords2ds: Vec<Coords2d>,
    pub loose_float_keys: Vec<FloatKeys>,
    pub loose_guids: Vec<GuidValue>,
    pub loose_indexed_colors: Vec<IndexedColor>,
    pub loose_matrix4x4s: Vec<Matrix4x4>,
    pub loose_mesh_faces: Vec<MeshFace>,
    pub loose_vectors: Vec<Vector>,
    pub loose_timed_float_keys: Vec<TimedFloatKeys>,
    pub frames: Vec<Frame>,
    pub loose_meshes: Vec<Mesh>,
    pub loose_patch_meshes: Vec<PatchMesh>,
    pub loose_materials: Vec<Material>,
    pub loose_effect_instances: Vec<EffectInstance>,
    pub animation_sets: Vec<AnimationSet>,
    pub compressed_animation_sets: Vec<CompressedAnimationSet>,
    pub unknown_objects: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub name: Option<String>,
    pub transform: Option<[f32; 16]>,
    pub meshes: Vec<Mesh>,
    pub patch_meshes: Vec<PatchMesh>,
    pub child_frames: Vec<Frame>,
    pub unknown_children: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub name: Option<String>,
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<Vec<u32>>,
    pub normals: Option<MeshNormals>,
    pub texcoords: Option<Vec<[f32; 2]>>,
    pub vertex_colors: Option<Vec<VertexColor>>,
    pub material_list: Option<MeshMaterialList>,
    pub face_adjacency: Option<FaceAdjacency>,
    pub mesh_face_wraps: Option<MeshFaceWraps>,
    pub vertex_duplication_indices: Option<VertexDuplicationIndices>,
    pub fvf_data: Option<FvfData>,
    pub decl_data: Option<DeclData>,
    pub skin_mesh_header: Option<XSkinMeshHeader>,
    pub skin_weights: Vec<SkinWeights>,
    pub effect_instances: Vec<EffectInstance>,
    pub pm_info: Option<PMInfo>,
    pub extras: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct MeshNormals {
    pub normals: Vec<[f32; 3]>,
    pub face_normals: Vec<Vec<u32>>,
}

#[derive(Debug, Clone, Default)]
pub struct VertexColor {
    pub index: u32,
    pub rgba: [f32; 4],
}

#[derive(Debug, Clone, Default)]
pub struct MeshMaterialList {
    pub material_count: u32,
    pub face_indexes: Vec<u32>,
    pub materials: Vec<Material>,
    pub material_references: Vec<ReferenceTarget>,
}

#[derive(Debug, Clone, Default)]
pub struct Material {
    pub name: Option<String>,
    pub face_color: [f32; 4],
    pub power: f32,
    pub specular_color: [f32; 3],
    pub emissive_color: [f32; 3],
    pub texture_filenames: Vec<String>,
    pub effect_instances: Vec<EffectInstance>,
    pub extras: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct Boolean {
    pub value: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Boolean2d {
    pub u: bool,
    pub v: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ColorRGB {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ColorRGBA {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Coords2d {
    pub u: f32,
    pub v: f32,
}

#[derive(Debug, Clone, Default)]
pub struct FloatKeys {
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct GuidValue {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[derive(Debug, Clone, Default)]
pub struct IndexedColor {
    pub index: u32,
    pub index_color: ColorRGBA,
}

#[derive(Debug, Clone, Default)]
pub struct Matrix4x4 {
    pub matrix: [f32; 16],
}

#[derive(Debug, Clone, Default)]
pub struct MeshFace {
    pub face_vertex_indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Default)]
pub struct MaterialWrap {
    pub u: bool,
    pub v: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MeshFaceWraps {
    pub wraps: Vec<MaterialWrap>,
}

#[derive(Debug, Clone, Default)]
pub struct FaceAdjacency {
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct VertexDuplicationIndices {
    pub original_vertices: u32,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct FvfData {
    pub fvf: u32,
    pub dwords: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct VertexElementDecl {
    pub type_code: u32,
    pub method: u32,
    pub usage: u32,
    pub usage_index: u32,
}

#[derive(Debug, Clone, Default)]
pub struct DeclData {
    pub elements: Vec<VertexElementDecl>,
    pub dwords: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct EffectString {
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct EffectDWord {
    pub value: u32,
}

#[derive(Debug, Clone, Default)]
pub struct EffectFloats {
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct EffectParamString {
    pub param_name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct EffectParamDWord {
    pub param_name: String,
    pub value: u32,
}

#[derive(Debug, Clone, Default)]
pub struct EffectParamFloats {
    pub param_name: String,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct EffectInstance {
    pub name: Option<String>,
    pub effect_filename: String,
    pub strings: Vec<EffectParamString>,
    pub dwords: Vec<EffectParamDWord>,
    pub floats: Vec<EffectParamFloats>,
    pub legacy_strings: Vec<EffectString>,
    pub legacy_dwords: Vec<EffectDWord>,
    pub legacy_floats: Vec<EffectFloats>,
    pub extras: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct Patch {
    pub control_indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct PMAttributeRange {
    pub face_offset: u32,
    pub faces_min: u32,
    pub faces_max: u32,
    pub vertex_offset: u32,
    pub vertices_min: u32,
    pub vertices_max: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PMVSplitRecord {
    pub face_clw: u32,
    pub vlr_offset: u32,
    pub code: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PMInfo {
    pub attribute_ranges: Vec<PMAttributeRange>,
    pub max_valence: u32,
    pub min_logical_vertices: u32,
    pub max_logical_vertices: u32,
    pub split_records: Vec<PMVSplitRecord>,
    pub attribute_mispredicts: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct PatchMesh {
    pub name: Option<String>,
    pub patch_type: Option<u32>,
    pub degree: Option<u32>,
    pub basis: Option<u32>,
    pub vertices: Vec<[f32; 3]>,
    pub patches: Vec<Patch>,
    pub effect_instances: Vec<EffectInstance>,
    pub pm_info: Option<PMInfo>,
    pub extras: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct CompressedAnimationSet {
    pub name: Option<String>,
    pub compressed_block_size: u32,
    pub ticks_per_sec: f32,
    pub playback_type: u32,
    pub buffer_length: u32,
    pub compressed_data: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct XSkinMeshHeader {
    pub max_skin_weights_per_vertex: u16,
    pub max_skin_weights_per_face: u16,
    pub bone_count: u16,
}

#[derive(Debug, Clone, Default)]
pub struct SkinWeights {
    pub transform_node_name: String,
    pub vertex_indices: Vec<u32>,
    pub weights: Vec<f32>,
    pub matrix_offset: [f32; 16],
}

#[derive(Debug, Clone, Default)]
pub struct AnimationSet {
    pub name: Option<String>,
    pub animations: Vec<Animation>,
}

#[derive(Debug, Clone, Default)]
pub struct Animation {
    pub name: Option<String>,
    pub target_name: Option<String>,
    pub target_uuid: Option<String>,
    pub keys: Vec<AnimationKeyBlock>,
    pub options: Option<AnimationOptions>,
    pub extras: Vec<XDataObject>,
}

#[derive(Debug, Clone, Default)]
pub struct AnimationOptions {
    pub open_closed: u32,
    pub position_quality: u32,
}

#[derive(Debug, Clone, Default)]
pub struct AnimationKeyBlock {
    pub key_type: u32,
    pub keys: Vec<TimedFloatKeys>,
}

#[derive(Debug, Clone, Default)]
pub struct TimedFloatKeys {
    pub time: u32,
    pub values: Vec<f32>,
}

impl Scene {
    pub fn from_xfile(file: &XFile) -> Result<Self> {
        let mut scene = Scene::default();
        for object in &file.objects {
            match object.class_name.as_str() {
                name if name.eq_ignore_ascii_case("AnimTicksPerSecond") => {
                    scene.anim_ticks_per_second = Some(parse_anim_ticks_per_second(object)?);
                }
                name if name.eq_ignore_ascii_case("Boolean") => {
                    scene.loose_booleans.push(parse_boolean(object)?);
                }
                name if name.eq_ignore_ascii_case("Boolean2d") => {
                    scene.loose_boolean2ds.push(parse_boolean2d(object)?);
                }
                name if name.eq_ignore_ascii_case("ColorRGB") => {
                    scene.loose_color_rgbs.push(parse_color_rgb(object)?);
                }
                name if name.eq_ignore_ascii_case("ColorRGBA") => {
                    scene.loose_color_rgbas.push(parse_color_rgba(object)?);
                }
                name if name.eq_ignore_ascii_case("Coords2d") => {
                    scene.loose_coords2ds.push(parse_coords2d(object)?);
                }
                name if name.eq_ignore_ascii_case("FloatKeys") => {
                    scene.loose_float_keys.push(parse_float_keys(object)?);
                }
                name if name.eq_ignore_ascii_case("Guid") => {
                    scene.loose_guids.push(parse_guid_value(object)?);
                }
                name if name.eq_ignore_ascii_case("IndexedColor") => {
                    scene.loose_indexed_colors.push(parse_indexed_color(object)?);
                }
                name if name.eq_ignore_ascii_case("Matrix4x4") => {
                    scene.loose_matrix4x4s.push(parse_matrix4x4(object)?);
                }
                name if name.eq_ignore_ascii_case("MeshFace") => {
                    scene.loose_mesh_faces.push(parse_mesh_face(object)?);
                }
                name if name.eq_ignore_ascii_case("Vector") => {
                    scene.loose_vectors.push(parse_vector(object)?);
                }
                name if name.eq_ignore_ascii_case("TimedFloatKeys") => {
                    scene.loose_timed_float_keys.push(parse_timed_float_keys(object)?);
                }
                name if name.eq_ignore_ascii_case("Frame") => {
                    scene.frames.push(parse_frame(object)?);
                }
                name if name.eq_ignore_ascii_case("Mesh") => {
                    scene.loose_meshes.push(parse_mesh(object)?);
                }
                name if name.eq_ignore_ascii_case("PatchMesh") || name.eq_ignore_ascii_case("PatchMesh9") => {
                    scene.loose_patch_meshes.push(parse_patch_mesh(object)?);
                }
                name if name.eq_ignore_ascii_case("Material") => {
                    scene.loose_materials.push(parse_material(object)?);
                }
                name if name.eq_ignore_ascii_case("EffectInstance") => {
                    scene.loose_effect_instances.push(parse_effect_instance(object)?);
                }
                name if name.eq_ignore_ascii_case("AnimationSet") => {
                    scene.animation_sets.push(parse_animation_set(object)?);
                }
                name if name.eq_ignore_ascii_case("CompressedAnimationSet") => {
                    scene.compressed_animation_sets.push(parse_compressed_animation_set(object)?);
                }
                _ => scene.unknown_objects.push(object.clone()),
            }
        }
        Ok(scene)
    }
}

fn parse_anim_ticks_per_second(object: &XDataObject) -> Result<u32> {
    let mut s = ScalarCursor::new(object);
    s.next_u32()
}

fn parse_frame(object: &XDataObject) -> Result<Frame> {
    let mut frame = Frame {
        name: object.object_name.clone(),
        ..Frame::default()
    };
    for child in object.elements.iter().filter_map(as_object) {
        match child.class_name.as_str() {
            name if name.eq_ignore_ascii_case("FrameTransformMatrix") => {
                frame.transform = Some(parse_frame_transform_matrix(child)?);
            }
            name if name.eq_ignore_ascii_case("Mesh") => frame.meshes.push(parse_mesh(child)?),
            name if name.eq_ignore_ascii_case("PatchMesh") || name.eq_ignore_ascii_case("PatchMesh9") => {
                frame.patch_meshes.push(parse_patch_mesh(child)?);
            }
            name if name.eq_ignore_ascii_case("Frame") => frame.child_frames.push(parse_frame(child)?),
            _ => frame.unknown_children.push(child.clone()),
        }
    }
    Ok(frame)
}

fn parse_frame_transform_matrix(object: &XDataObject) -> Result<[f32; 16]> {
    let mut s = ScalarCursor::new(object);
    let mut out = [0.0f32; 16];
    for item in &mut out {
        *item = s.next_f32()?;
    }
    Ok(out)
}

fn parse_mesh(object: &XDataObject) -> Result<Mesh> {
    let mut s = ScalarCursor::new(object);
    let vertex_count = s.next_usize()?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push([s.next_f32()?, s.next_f32()?, s.next_f32()?]);
    }
    let face_count = s.next_usize()?;
    let mut faces = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let n = s.next_usize()?;
        let mut face = Vec::with_capacity(n);
        for _ in 0..n {
            face.push(s.next_u32()?);
        }
        faces.push(face);
    }

    let mut mesh = Mesh {
        name: object.object_name.clone(),
        vertices,
        faces,
        ..Mesh::default()
    };

    for child in object.elements.iter().filter_map(as_object) {
        match child.class_name.as_str() {
            name if name.eq_ignore_ascii_case("MeshNormals") => mesh.normals = Some(parse_mesh_normals(child)?),
            name if name.eq_ignore_ascii_case("MeshTextureCoords") => mesh.texcoords = Some(parse_mesh_texcoords(child)?),
            name if name.eq_ignore_ascii_case("MeshVertexColors") => mesh.vertex_colors = Some(parse_mesh_vertex_colors(child)?),
            name if name.eq_ignore_ascii_case("MeshMaterialList") => mesh.material_list = Some(parse_mesh_material_list(child)?),
            name if name.eq_ignore_ascii_case("FaceAdjacency") => mesh.face_adjacency = Some(parse_face_adjacency(child)?),
            name if name.eq_ignore_ascii_case("MeshFaceWraps") => mesh.mesh_face_wraps = Some(parse_mesh_face_wraps(child)?),
            name if name.eq_ignore_ascii_case("VertexDuplicationIndices") => {
                mesh.vertex_duplication_indices = Some(parse_vertex_duplication_indices(child)?);
            }
            name if name.eq_ignore_ascii_case("FVFData") => mesh.fvf_data = Some(parse_fvf_data(child)?),
            name if name.eq_ignore_ascii_case("DeclData") => mesh.decl_data = Some(parse_decl_data(child)?),
            name if name.eq_ignore_ascii_case("XSkinMeshHeader") => mesh.skin_mesh_header = Some(parse_xskin_mesh_header(child)?),
            name if name.eq_ignore_ascii_case("SkinWeights") => mesh.skin_weights.push(parse_skin_weights(child)?),
            name if name.eq_ignore_ascii_case("EffectInstance") => mesh.effect_instances.push(parse_effect_instance(child)?),
            name if name.eq_ignore_ascii_case("PMInfo") => mesh.pm_info = Some(parse_pm_info(child)?),
            _ => mesh.extras.push(child.clone()),
        }
    }

    normalize_mesh_for_common_exporter_quirks(&mut mesh);
    Ok(mesh)
}

fn normalize_mesh_for_common_exporter_quirks(mesh: &mut Mesh) {
    if let Some(material_list) = &mut mesh.material_list {
        let face_count = mesh.faces.len();
        let payload_count = (material_list.materials.len() + material_list.material_references.len()) as u32;
        if material_list.material_count < payload_count {
            material_list.material_count = payload_count;
        }
        if material_list.face_indexes.len() == 1 && face_count > 1 {
            let index = material_list.face_indexes[0];
            material_list.face_indexes.resize(face_count, index);
        } else if material_list.material_count == 1 && material_list.face_indexes.is_empty() && face_count != 0 {
            material_list.face_indexes.resize(face_count, 0);
        }
    }
}

fn parse_mesh_normals(object: &XDataObject) -> Result<MeshNormals> {
    let mut s = ScalarCursor::new(object);
    let normal_count = s.next_usize()?;
    let mut normals = Vec::with_capacity(normal_count);
    for _ in 0..normal_count {
        normals.push([s.next_f32()?, s.next_f32()?, s.next_f32()?]);
    }
    let face_count = s.next_usize()?;
    let mut face_normals = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        let n = s.next_usize()?;
        let mut face = Vec::with_capacity(n);
        for _ in 0..n {
            face.push(s.next_u32()?);
        }
        face_normals.push(face);
    }
    Ok(MeshNormals { normals, face_normals })
}

fn parse_mesh_texcoords(object: &XDataObject) -> Result<Vec<[f32; 2]>> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut coords = Vec::with_capacity(count);
    for _ in 0..count {
        coords.push([s.next_f32()?, s.next_f32()?]);
    }
    Ok(coords)
}

fn parse_mesh_vertex_colors(object: &XDataObject) -> Result<Vec<VertexColor>> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut colors = Vec::with_capacity(count);
    for _ in 0..count {
        colors.push(VertexColor {
            index: s.next_u32()?,
            rgba: [s.next_f32()?, s.next_f32()?, s.next_f32()?, s.next_f32()?],
        });
    }
    Ok(colors)
}

fn parse_mesh_material_list(object: &XDataObject) -> Result<MeshMaterialList> {
    let mut s = ScalarCursor::new(object);
    let material_count = s.next_u32()?;
    let face_index_count = s.next_usize()?;
    let mut face_indexes = Vec::with_capacity(face_index_count);
    for _ in 0..face_index_count {
        face_indexes.push(s.next_u32()?);
    }

    let mut out = MeshMaterialList {
        material_count,
        face_indexes,
        ..MeshMaterialList::default()
    };

    for element in &object.elements {
        match element {
            XObjectElement::NestedObject(child) if child.class_name.eq_ignore_ascii_case("Material") => {
                out.materials.push(parse_material(child)?);
            }
            XObjectElement::Reference(reference) => out.material_references.push(reference.clone()),
            _ => {}
        }
    }

    Ok(out)
}

fn parse_boolean(object: &XDataObject) -> Result<Boolean> {
    let mut s = ScalarCursor::new(object);
    Ok(Boolean { value: s.next_bool()? })
}

fn parse_boolean2d(object: &XDataObject) -> Result<Boolean2d> {
    let mut s = ScalarCursor::new(object);
    Ok(Boolean2d {
        u: s.next_bool()?,
        v: s.next_bool()?,
    })
}

fn parse_color_rgb(object: &XDataObject) -> Result<ColorRGB> {
    let mut s = ScalarCursor::new(object);
    Ok(ColorRGB {
        red: s.next_f32()?,
        green: s.next_f32()?,
        blue: s.next_f32()?,
    })
}

fn parse_color_rgba(object: &XDataObject) -> Result<ColorRGBA> {
    let mut s = ScalarCursor::new(object);
    Ok(ColorRGBA {
        red: s.next_f32()?,
        green: s.next_f32()?,
        blue: s.next_f32()?,
        alpha: s.next_f32()?,
    })
}

fn parse_coords2d(object: &XDataObject) -> Result<Coords2d> {
    let mut s = ScalarCursor::new(object);
    Ok(Coords2d {
        u: s.next_f32()?,
        v: s.next_f32()?,
    })
}

fn parse_float_keys(object: &XDataObject) -> Result<FloatKeys> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(s.next_f32()?);
    }
    Ok(FloatKeys { values })
}

fn parse_guid_value(object: &XDataObject) -> Result<GuidValue> {
    let mut s = ScalarCursor::new(object);
    let data1 = s.next_u32()?;
    let data2 = s.next_u16()?;
    let data3 = s.next_u16()?;
    let mut data4 = [0u8; 8];
    for item in &mut data4 {
        *item = s.next_u8()?;
    }
    Ok(GuidValue {
        data1,
        data2,
        data3,
        data4,
    })
}

fn parse_indexed_color(object: &XDataObject) -> Result<IndexedColor> {
    let mut s = ScalarCursor::new(object);
    Ok(IndexedColor {
        index: s.next_u32()?,
        index_color: ColorRGBA {
            red: s.next_f32()?,
            green: s.next_f32()?,
            blue: s.next_f32()?,
            alpha: s.next_f32()?,
        },
    })
}

fn parse_matrix4x4(object: &XDataObject) -> Result<Matrix4x4> {
    let mut s = ScalarCursor::new(object);
    let mut matrix = [0.0f32; 16];
    for item in &mut matrix {
        *item = s.next_f32()?;
    }
    Ok(Matrix4x4 { matrix })
}

fn parse_mesh_face(object: &XDataObject) -> Result<MeshFace> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut face_vertex_indices = Vec::with_capacity(count);
    for _ in 0..count {
        face_vertex_indices.push(s.next_u32()?);
    }
    Ok(MeshFace { face_vertex_indices })
}

fn parse_vector(object: &XDataObject) -> Result<Vector> {
    let mut s = ScalarCursor::new(object);
    Ok(Vector {
        x: s.next_f32()?,
        y: s.next_f32()?,
        z: s.next_f32()?,
    })
}

fn parse_timed_float_keys(object: &XDataObject) -> Result<TimedFloatKeys> {
    let mut s = ScalarCursor::new(object);
    let time = s.next_u32()?;
    let key_count = s.next_usize()?;
    let mut values = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        values.push(s.next_f32()?);
    }
    Ok(TimedFloatKeys { time, values })
}

fn parse_material(object: &XDataObject) -> Result<Material> {
    let mut s = ScalarCursor::new(object);
    let mut material = Material {
        name: object.object_name.clone(),
        face_color: [s.next_f32()?, s.next_f32()?, s.next_f32()?, s.next_f32()?],
        power: s.next_f32()?,
        specular_color: [s.next_f32()?, s.next_f32()?, s.next_f32()?],
        emissive_color: [s.next_f32()?, s.next_f32()?, s.next_f32()?],
        ..Material::default()
    };

    for child in object.elements.iter().filter_map(as_object) {
        match child.class_name.as_str() {
            name
                if name.eq_ignore_ascii_case("TextureFilename")
                    || name.eq_ignore_ascii_case("TextureFileName")
                    || name.eq_ignore_ascii_case("NormalmapFilename")
                    || name.eq_ignore_ascii_case("NormalmapFileName") =>
            {
                let texture = parse_texture_filename(child)?;
                if !texture.is_empty() {
                    material.texture_filenames.push(texture);
                }
            }
            name if name.eq_ignore_ascii_case("EffectInstance") => {
                material.effect_instances.push(parse_effect_instance(child)?);
            }
            _ => material.extras.push(child.clone()),
        }
    }

    Ok(material)
}

fn parse_texture_filename(object: &XDataObject) -> Result<String> {
    let mut s = ScalarCursor::new(object);
    let name = s.next_string()?;
    Ok(normalize_texture_path(name))
}

fn normalize_texture_path(mut path: String) -> String {
    while path.contains("\\\\") {
        path = path.replace("\\\\", "\\");
    }
    path
}

fn parse_face_adjacency(object: &XDataObject) -> Result<FaceAdjacency> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
        indices.push(s.next_u32()?);
    }
    Ok(FaceAdjacency { indices })
}

fn parse_material_wrap(object: &XDataObject) -> Result<MaterialWrap> {
    let mut s = ScalarCursor::new(object);
    Ok(MaterialWrap {
        u: s.next_bool()?,
        v: s.next_bool()?,
    })
}

fn parse_mesh_face_wraps(object: &XDataObject) -> Result<MeshFaceWraps> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut wraps = Vec::with_capacity(count);
    for _ in 0..count {
        wraps.push(MaterialWrap {
            u: s.next_bool()?,
            v: s.next_bool()?,
        });
    }
    if wraps.is_empty() {
        for child in object.elements.iter().filter_map(as_object) {
            if child.class_name.eq_ignore_ascii_case("MaterialWrap") {
                wraps.push(parse_material_wrap(child)?);
            }
        }
    }
    Ok(MeshFaceWraps { wraps })
}

fn parse_vertex_duplication_indices(object: &XDataObject) -> Result<VertexDuplicationIndices> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let original_vertices = s.next_u32()?;
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
        indices.push(s.next_u32()?);
    }
    Ok(VertexDuplicationIndices {
        original_vertices,
        indices,
    })
}

fn parse_fvf_data(object: &XDataObject) -> Result<FvfData> {
    let mut s = ScalarCursor::new(object);
    let fvf = s.next_u32()?;
    let count = s.next_usize()?;
    let mut dwords = Vec::with_capacity(count);
    for _ in 0..count {
        dwords.push(s.next_u32()?);
    }
    Ok(FvfData { fvf, dwords })
}

fn parse_decl_data(object: &XDataObject) -> Result<DeclData> {
    let mut s = ScalarCursor::new(object);
    let element_count = s.next_usize()?;
    let mut elements = Vec::with_capacity(element_count);
    for _ in 0..element_count {
        elements.push(VertexElementDecl {
            type_code: s.next_u32()?,
            method: s.next_u32()?,
            usage: s.next_u32()?,
            usage_index: s.next_u32()?,
        });
    }
    let data_count = s.next_usize()?;
    let mut dwords = Vec::with_capacity(data_count);
    for _ in 0..data_count {
        dwords.push(s.next_u32()?);
    }
    Ok(DeclData { elements, dwords })
}

fn parse_effect_string(object: &XDataObject) -> Result<EffectString> {
    let mut s = ScalarCursor::new(object);
    Ok(EffectString { value: s.next_string()? })
}

fn parse_effect_dword(object: &XDataObject) -> Result<EffectDWord> {
    let mut s = ScalarCursor::new(object);
    Ok(EffectDWord { value: s.next_u32()? })
}

fn parse_effect_floats(object: &XDataObject) -> Result<EffectFloats> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(s.next_f32()?);
    }
    Ok(EffectFloats { values })
}

fn parse_effect_param_string(object: &XDataObject) -> Result<EffectParamString> {
    let mut s = ScalarCursor::new(object);
    Ok(EffectParamString {
        param_name: s.next_string()?,
        value: s.next_string()?,
    })
}

fn parse_effect_param_dword(object: &XDataObject) -> Result<EffectParamDWord> {
    let mut s = ScalarCursor::new(object);
    Ok(EffectParamDWord {
        param_name: s.next_string()?,
        value: s.next_u32()?,
    })
}

fn parse_effect_param_floats(object: &XDataObject) -> Result<EffectParamFloats> {
    let mut s = ScalarCursor::new(object);
    let param_name = s.next_string()?;
    let count = s.next_usize()?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(s.next_f32()?);
    }
    Ok(EffectParamFloats { param_name, values })
}

fn parse_effect_instance(object: &XDataObject) -> Result<EffectInstance> {
    let mut s = ScalarCursor::new(object);
    let mut out = EffectInstance {
        name: object.object_name.clone(),
        effect_filename: s.next_string()?,
        ..EffectInstance::default()
    };
    for child in object.elements.iter().filter_map(as_object) {
        match child.class_name.as_str() {
            name if name.eq_ignore_ascii_case("EffectParamString") => out.strings.push(parse_effect_param_string(child)?),
            name if name.eq_ignore_ascii_case("EffectParamDWord") => out.dwords.push(parse_effect_param_dword(child)?),
            name if name.eq_ignore_ascii_case("EffectParamFloats") => out.floats.push(parse_effect_param_floats(child)?),
            name if name.eq_ignore_ascii_case("EffectString") => out.legacy_strings.push(parse_effect_string(child)?),
            name if name.eq_ignore_ascii_case("EffectDWord") => out.legacy_dwords.push(parse_effect_dword(child)?),
            name if name.eq_ignore_ascii_case("EffectFloats") => out.legacy_floats.push(parse_effect_floats(child)?),
            _ => out.extras.push(child.clone()),
        }
    }
    Ok(out)
}

fn parse_patch(object: &XDataObject) -> Result<Patch> {
    let mut s = ScalarCursor::new(object);
    let count = s.next_usize()?;
    let mut control_indices = Vec::with_capacity(count);
    for _ in 0..count {
        control_indices.push(s.next_u32()?);
    }
    Ok(Patch { control_indices })
}

fn parse_pm_attribute_range(object: &XDataObject) -> Result<PMAttributeRange> {
    let mut s = ScalarCursor::new(object);
    Ok(PMAttributeRange {
        face_offset: s.next_u32()?,
        faces_min: s.next_u32()?,
        faces_max: s.next_u32()?,
        vertex_offset: s.next_u32()?,
        vertices_min: s.next_u32()?,
        vertices_max: s.next_u32()?,
    })
}

fn parse_pmv_split_record(object: &XDataObject) -> Result<PMVSplitRecord> {
    let mut s = ScalarCursor::new(object);
    Ok(PMVSplitRecord {
        face_clw: s.next_u32()?,
        vlr_offset: s.next_u32()?,
        code: s.next_u32()?,
    })
}

fn parse_pm_info(object: &XDataObject) -> Result<PMInfo> {
    let mut s = ScalarCursor::new(object);
    let attribute_count = s.next_usize()?;
    let mut attribute_ranges = Vec::new();
    for child in object.nested_objects_named("PMAttributeRange").take(attribute_count) {
        attribute_ranges.push(parse_pm_attribute_range(child)?);
    }
    let max_valence = s.next_u32()?;
    let min_logical_vertices = s.next_u32()?;
    let max_logical_vertices = s.next_u32()?;
    let split_count = s.next_usize()?;
    let mut split_records = Vec::new();
    for child in object.nested_objects_named("PMVSplitRecord").take(split_count) {
        split_records.push(parse_pmv_split_record(child)?);
    }
    let mispredict_count = s.next_usize()?;
    let mut attribute_mispredicts = Vec::with_capacity(mispredict_count);
    for _ in 0..mispredict_count {
        attribute_mispredicts.push(s.next_u32()?);
    }
    Ok(PMInfo {
        attribute_ranges,
        max_valence,
        min_logical_vertices,
        max_logical_vertices,
        split_records,
        attribute_mispredicts,
    })
}

fn parse_patch_mesh(object: &XDataObject) -> Result<PatchMesh> {
    let is_patch_mesh9 = object.class_name.eq_ignore_ascii_case("PatchMesh9");
    let mut s = ScalarCursor::new(object);
    let (patch_type, degree, basis) = if is_patch_mesh9 {
        (Some(s.next_u32()?), Some(s.next_u32()?), Some(s.next_u32()?))
    } else {
        (None, None, None)
    };
    let vertex_count = s.next_usize()?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push([s.next_f32()?, s.next_f32()?, s.next_f32()?]);
    }
    let _patch_count = s.next_usize()?;

    let mut out = PatchMesh {
        name: object.object_name.clone(),
        patch_type,
        degree,
        basis,
        vertices,
        ..PatchMesh::default()
    };

    for child in object.elements.iter().filter_map(as_object) {
        match child.class_name.as_str() {
            name if name.eq_ignore_ascii_case("Patch") => out.patches.push(parse_patch(child)?),
            name if name.eq_ignore_ascii_case("EffectInstance") => out.effect_instances.push(parse_effect_instance(child)?),
            name if name.eq_ignore_ascii_case("PMInfo") => out.pm_info = Some(parse_pm_info(child)?),
            _ => out.extras.push(child.clone()),
        }
    }
    Ok(out)
}

fn parse_compressed_animation_set(object: &XDataObject) -> Result<CompressedAnimationSet> {
    let mut s = ScalarCursor::new(object);
    let compressed_block_size = s.next_u32()?;
    let ticks_per_sec = s.next_f32()?;
    let playback_type = s.next_u32()?;
    let buffer_length = s.next_u32()?;
    let mut compressed_data = Vec::with_capacity(buffer_length as usize);
    for _ in 0..buffer_length {
        compressed_data.push(s.next_u32()?);
    }
    Ok(CompressedAnimationSet {
        name: object.object_name.clone(),
        compressed_block_size,
        ticks_per_sec,
        playback_type,
        buffer_length,
        compressed_data,
    })
}

fn parse_xskin_mesh_header(object: &XDataObject) -> Result<XSkinMeshHeader> {
    let mut s = ScalarCursor::new(object);
    Ok(XSkinMeshHeader {
        max_skin_weights_per_vertex: s.next_u16()?,
        max_skin_weights_per_face: s.next_u16()?,
        bone_count: s.next_u16()?,
    })
}

fn parse_skin_weights(object: &XDataObject) -> Result<SkinWeights> {
    let mut s = ScalarCursor::new(object);
    let transform_node_name = s.next_string()?;
    let weight_count = s.next_usize()?;
    let mut vertex_indices = Vec::with_capacity(weight_count);
    for _ in 0..weight_count {
        vertex_indices.push(s.next_u32()?);
    }
    let mut weights = Vec::with_capacity(weight_count);
    for _ in 0..weight_count {
        weights.push(s.next_f32()?);
    }
    let mut matrix_offset = [0.0f32; 16];
    for item in &mut matrix_offset {
        *item = s.next_f32()?;
    }
    Ok(SkinWeights {
        transform_node_name,
        vertex_indices,
        weights,
        matrix_offset,
    })
}

fn parse_animation_set(object: &XDataObject) -> Result<AnimationSet> {
    let mut set = AnimationSet {
        name: object.object_name.clone(),
        ..AnimationSet::default()
    };
    for child in object.elements.iter().filter_map(as_object) {
        if child.class_name.eq_ignore_ascii_case("Animation") {
            set.animations.push(parse_animation(child)?);
        }
    }
    Ok(set)
}

fn parse_animation(object: &XDataObject) -> Result<Animation> {
    let mut animation = Animation {
        name: object.object_name.clone(),
        ..Animation::default()
    };
    for element in &object.elements {
        match element {
            XObjectElement::Reference(reference) => {
                if animation.target_name.is_none() {
                    animation.target_name = reference.name.clone();
                }
                if animation.target_uuid.is_none() {
                    animation.target_uuid = reference.uuid.as_ref().map(|g| g.to_string());
                }
            }
            XObjectElement::NestedObject(child) if child.class_name.eq_ignore_ascii_case("AnimationKey") => {
                animation.keys.push(parse_animation_key(child)?);
            }
            XObjectElement::NestedObject(child) if child.class_name.eq_ignore_ascii_case("AnimationOptions") => {
                animation.options = Some(parse_animation_options(child)?);
            }
            XObjectElement::NestedObject(child) => animation.extras.push(child.clone()),
            _ => {}
        }
    }
    Ok(animation)
}

fn parse_animation_options(object: &XDataObject) -> Result<AnimationOptions> {
    let mut s = ScalarCursor::new(object);
    Ok(AnimationOptions {
        open_closed: s.next_u32()?,
        position_quality: s.next_u32()?,
    })
}

fn parse_animation_key(object: &XDataObject) -> Result<AnimationKeyBlock> {
    let mut s = ScalarCursor::new(object);
    let key_type = s.next_u32()?;
    let key_count = s.next_usize()?;
    let mut keys = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        let time = s.next_u32()?;
        let value_count = s.next_usize()?;
        let expected = match key_type {
            0 => 4,
            1 | 2 => 3,
            3 | 4 => 16,
            other => {
                return Err(Error::Semantic(format!(
                    "unknown AnimationKey key_type {}",
                    other
                )))
            }
        };
        if value_count != expected {
            return Err(Error::Semantic(format!(
                "AnimationKey key_type {} expected {} values, got {}",
                key_type, expected, value_count
            )));
        }
        let mut values = Vec::with_capacity(value_count);
        for _ in 0..value_count {
            values.push(s.next_f32()?);
        }
        keys.push(TimedFloatKeys { time, values });
    }
    Ok(AnimationKeyBlock { key_type, keys })
}

fn parse_exporter_float_identifier(raw: &str) -> Result<f32> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("#ind") || lower.contains("#qnan") || lower == "nan" || lower == "+nan" || lower == "-nan" {
        return Ok(0.0);
    }
    if lower == "inf" || lower == "+inf" || lower == "infinity" || lower == "+infinity" {
        return Ok(f32::INFINITY);
    }
    if lower == "-inf" || lower == "-infinity" {
        return Ok(f32::NEG_INFINITY);
    }
    Err(Error::Semantic(format!("expected float, got identifier {raw:?}")))
}

fn as_object(element: &XObjectElement) -> Option<&XDataObject> {
    match element {
        XObjectElement::NestedObject(object) => Some(object),
        _ => None,
    }
}

struct ScalarCursor<'a> {
    values: Vec<&'a PrimitiveValue>,
    cursor: usize,
}

impl<'a> ScalarCursor<'a> {
    fn new(object: &'a XDataObject) -> Self {
        let values = object
            .elements
            .iter()
            .filter_map(|element| match element {
                XObjectElement::Primitive(v) => Some(v),
                _ => None,
            })
            .collect();
        Self { values, cursor: 0 }
    }

    fn next(&mut self) -> Result<&'a PrimitiveValue> {
        let value = self.values.get(self.cursor).copied().ok_or_else(|| {
            Error::Semantic("unexpected end of scalar stream while lifting semantic object".to_string())
        })?;
        self.cursor += 1;
        Ok(value)
    }

    fn next_u32(&mut self) -> Result<u32> {
        match self.next()? {
            PrimitiveValue::Integer(v) if *v >= 0 => Ok(*v as u32),
            other => Err(Error::Semantic(format!("expected u32, got {:?}", other))),
        }
    }

    fn next_u16(&mut self) -> Result<u16> {
        let value = self.next_u32()?;
        u16::try_from(value).map_err(|_| Error::Semantic(format!("u16 overflow: {value}")))
    }

    fn next_u8(&mut self) -> Result<u8> {
        let value = self.next_u32()?;
        u8::try_from(value).map_err(|_| Error::Semantic(format!("u8 overflow: {value}")))
    }

    fn next_usize(&mut self) -> Result<usize> {
        usize::try_from(self.next_u32()?).map_err(|_| Error::Semantic("usize overflow".to_string()))
    }

    fn next_bool(&mut self) -> Result<bool> {
        Ok(self.next_u32()? != 0)
    }

    fn next_f32(&mut self) -> Result<f32> {
        match self.next()? {
            PrimitiveValue::Float(v) => Ok(*v as f32),
            PrimitiveValue::Integer(v) => Ok(*v as f32),
            PrimitiveValue::Identifier(s) => parse_exporter_float_identifier(s),
            other => Err(Error::Semantic(format!("expected float, got {:?}", other))),
        }
    }

    fn next_string(&mut self) -> Result<String> {
        match self.next()? {
            PrimitiveValue::String(s) => Ok(s.clone()),
            PrimitiveValue::Identifier(s) => Ok(s.clone()),
            other => Err(Error::Semantic(format!("expected string, got {:?}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{FloatSize, FormatKind, XFileHeader};
    use crate::model::{PrimitiveValue, XFile, XObjectElement};

    fn header() -> XFileHeader {
        XFileHeader {
            major: 3,
            minor: 3,
            format: FormatKind::Text,
            float_size: FloatSize::F32,
        }
    }

    fn primitive_string(name: &str, value: &str) -> XDataObject {
        XDataObject {
            class_name: name.to_string(),
            object_name: None,
            class_id: None,
            elements: vec![XObjectElement::Primitive(PrimitiveValue::String(value.to_string()))],
        }
    }

    #[test]
    fn material_accepts_texture_filename_variants_and_normalizes_backslashes() {
        let material = XDataObject {
            class_name: "Material".to_string(),
            object_name: Some("Mat".to_string()),
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(8.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::NestedObject(primitive_string("TextureFileName", r"a\b\diffuse.png")),
                XObjectElement::NestedObject(primitive_string("NormalmapFileName", r"a\b\normal.png")),
            ],
        };

        let scene = Scene::from_xfile(&XFile {
            header: header(),
            templates: vec![],
            objects: vec![material],
        })
        .unwrap();

        assert_eq!(scene.loose_materials.len(), 1);
        assert_eq!(
            scene.loose_materials[0].texture_filenames,
            vec![r"a\b\diffuse.png", r"a\b\normal.png"]
        );
    }

    #[test]
    fn animation_key_rejects_wrong_arity() {
        let animation_key = XDataObject {
            class_name: "AnimationKey".to_string(),
            object_name: None,
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
            ],
        };

        let err = parse_animation_key(&animation_key).unwrap_err();
        assert!(format!("{}", err).contains("expected 4 values"));
    }

    #[test]
    fn effect_instance_lifts_named_and_legacy_parameters() {
        let effect = XDataObject {
            class_name: "EffectInstance".to_string(),
            object_name: Some("Fx0".to_string()),
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::String("shader.fx".to_string())),
                XObjectElement::NestedObject(XDataObject {
                    class_name: "EffectParamString".to_string(),
                    object_name: None,
                    class_id: None,
                    elements: vec![
                        XObjectElement::Primitive(PrimitiveValue::String("DiffuseMap".to_string())),
                        XObjectElement::Primitive(PrimitiveValue::String("diffuse.png".to_string())),
                    ],
                }),
                XObjectElement::NestedObject(XDataObject {
                    class_name: "EffectDWord".to_string(),
                    object_name: None,
                    class_id: None,
                    elements: vec![XObjectElement::Primitive(PrimitiveValue::Integer(7))],
                }),
            ],
        };

        let scene = Scene::from_xfile(&XFile {
            header: header(),
            templates: vec![],
            objects: vec![effect],
        })
        .unwrap();

        assert_eq!(scene.loose_effect_instances.len(), 1);
        assert_eq!(scene.loose_effect_instances[0].effect_filename, "shader.fx");
        assert_eq!(scene.loose_effect_instances[0].strings[0].param_name, "DiffuseMap");
        assert_eq!(scene.loose_effect_instances[0].legacy_dwords[0].value, 7);
    }

    #[test]
    fn patch_mesh9_lifts_patch_payload() {
        let patch_mesh = XDataObject {
            class_name: "PatchMesh9".to_string(),
            object_name: Some("Patchy".to_string()),
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(4)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                XObjectElement::NestedObject(XDataObject {
                    class_name: "Patch".to_string(),
                    object_name: None,
                    class_id: None,
                    elements: vec![
                        XObjectElement::Primitive(PrimitiveValue::Integer(4)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(2)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                    ],
                }),
            ],
        };

        let scene = Scene::from_xfile(&XFile {
            header: header(),
            templates: vec![],
            objects: vec![patch_mesh],
        })
        .unwrap();

        assert_eq!(scene.loose_patch_meshes.len(), 1);
        assert_eq!(scene.loose_patch_meshes[0].patch_type, Some(1));
        assert_eq!(scene.loose_patch_meshes[0].patches[0].control_indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn compressed_animation_set_lifts_header_and_payload() {
        let object = XDataObject {
            class_name: "CompressedAnimationSet".to_string(),
            object_name: Some("Cas".to_string()),
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::Integer(16)),
                XObjectElement::Primitive(PrimitiveValue::Float(30.0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(2)),
                XObjectElement::Primitive(PrimitiveValue::Integer(4)),
                XObjectElement::Primitive(PrimitiveValue::Integer(10)),
                XObjectElement::Primitive(PrimitiveValue::Integer(20)),
                XObjectElement::Primitive(PrimitiveValue::Integer(30)),
                XObjectElement::Primitive(PrimitiveValue::Integer(40)),
            ],
        };

        let scene = Scene::from_xfile(&XFile {
            header: header(),
            templates: vec![],
            objects: vec![object],
        })
        .unwrap();

        assert_eq!(scene.compressed_animation_sets.len(), 1);
        assert_eq!(scene.compressed_animation_sets[0].compressed_block_size, 16);
        assert_eq!(scene.compressed_animation_sets[0].compressed_data, vec![10, 20, 30, 40]);
    }

    #[test]
    fn mesh_lifts_face_adjacency_and_duplication_indices() {
        let mesh = XDataObject {
            class_name: "Mesh".to_string(),
            object_name: Some("M".to_string()),
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                XObjectElement::Primitive(PrimitiveValue::Integer(2)),
                XObjectElement::NestedObject(XDataObject {
                    class_name: "FaceAdjacency".to_string(),
                    object_name: None,
                    class_id: None,
                    elements: vec![
                        XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                    ],
                }),
                XObjectElement::NestedObject(XDataObject {
                    class_name: "VertexDuplicationIndices".to_string(),
                    object_name: None,
                    class_id: None,
                    elements: vec![
                        XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(2)),
                    ],
                }),
            ],
        };

        let scene = Scene::from_xfile(&XFile {
            header: header(),
            templates: vec![],
            objects: vec![mesh],
        })
        .unwrap();

        let mesh = &scene.loose_meshes[0];
        assert_eq!(mesh.face_adjacency.as_ref().unwrap().indices.len(), 3);
        assert_eq!(mesh.vertex_duplication_indices.as_ref().unwrap().indices, vec![0, 1, 2]);
    }


    #[test]
    fn parses_helper_templates_as_structured_scene_objects() {
        let sample = b"xof 0303txt 0032
Boolean Bool0 { 1; }
Boolean2d Wrap0 { 1; 0; }
ColorRGB Diffuse { 0.25; 0.5; 0.75; }
ColorRGBA Tint { 0.1; 0.2; 0.3; 0.4; }
Coords2d UV0 { 0.125; 0.875; }
FloatKeys Keys0 { 3; 1.0, 2.0, 3.0;; }
Guid Guid0 { 305419896; 4660; 22136; 1,2,3,4,5,6,7,8;; }
IndexedColor VC0 { 9; 0.5; 0.6; 0.7; 0.8;; }
Matrix4x4 M0 {
  1.0;0.0;0.0;0.0;,
  0.0;1.0;0.0;0.0;,
  0.0;0.0;1.0;0.0;,
  0.0;0.0;0.0;1.0;;
}
MeshFace F0 { 3; 0,1,2;; }
TimedFloatKeys TK0 { 10; 4; 1.0,2.0,3.0,4.0;; }
Vector V0 { 1.0; 2.0; 3.0; }
";
        let file = parse_x(sample).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        assert_eq!(scene.loose_booleans[0].value, true);
        assert_eq!(scene.loose_boolean2ds[0].u, true);
        assert_eq!(scene.loose_boolean2ds[0].v, false);
        assert_eq!(scene.loose_color_rgbs[0].blue, 0.75);
        assert_eq!(scene.loose_color_rgbas[0].alpha, 0.4);
        assert_eq!(scene.loose_coords2ds[0].v, 0.875);
        assert_eq!(scene.loose_float_keys[0].values, vec![1.0, 2.0, 3.0]);
        assert_eq!(scene.loose_guids[0].data4, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(scene.loose_indexed_colors[0].index, 9);
        assert_eq!(scene.loose_matrix4x4s[0].matrix[15], 1.0);
        assert_eq!(scene.loose_mesh_faces[0].face_vertex_indices, vec![0, 1, 2]);
        assert_eq!(scene.loose_timed_float_keys[0].time, 10);
        assert_eq!(scene.loose_timed_float_keys[0].values.len(), 4);
        assert_eq!(scene.loose_vectors[0].z, 3.0);
    }


    #[test]
    fn mesh_material_single_face_index_is_replicated() {
        let mesh = XDataObject {
            class_name: "Mesh".to_string(),
            object_name: Some("M".to_string()),
            class_id: None,
            elements: vec![
                XObjectElement::Primitive(PrimitiveValue::Integer(4)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)), XObjectElement::Primitive(PrimitiveValue::Float(0.0)), XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)), XObjectElement::Primitive(PrimitiveValue::Float(0.0)), XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(1.0)), XObjectElement::Primitive(PrimitiveValue::Float(1.0)), XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Float(0.0)), XObjectElement::Primitive(PrimitiveValue::Float(1.0)), XObjectElement::Primitive(PrimitiveValue::Float(0.0)),
                XObjectElement::Primitive(PrimitiveValue::Integer(2)),
                XObjectElement::Primitive(PrimitiveValue::Integer(3)), XObjectElement::Primitive(PrimitiveValue::Integer(0)), XObjectElement::Primitive(PrimitiveValue::Integer(1)), XObjectElement::Primitive(PrimitiveValue::Integer(2)),
                XObjectElement::Primitive(PrimitiveValue::Integer(3)), XObjectElement::Primitive(PrimitiveValue::Integer(0)), XObjectElement::Primitive(PrimitiveValue::Integer(2)), XObjectElement::Primitive(PrimitiveValue::Integer(3)),
                XObjectElement::NestedObject(XDataObject {
                    class_name: "MeshMaterialList".to_string(),
                    object_name: None,
                    class_id: None,
                    elements: vec![
                        XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(1)),
                        XObjectElement::Primitive(PrimitiveValue::Integer(0)),
                    ],
                }),
            ],
        };

        let scene = Scene::from_xfile(&XFile {
            header: header(),
            templates: vec![],
            objects: vec![mesh],
        })
        .unwrap();
        assert_eq!(scene.loose_meshes[0].material_list.as_ref().unwrap().face_indexes, vec![0, 0]);
    }

}
