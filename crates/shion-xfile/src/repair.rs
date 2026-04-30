use crate::semantic::{Animation, Frame, Mesh, MeshMaterialList, Scene, SkinWeights};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairMessage {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct RepairReport {
    pub messages: Vec<RepairMessage>,
}

impl RepairReport {
    pub fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.messages.push(RepairMessage {
            path: path.into(),
            message: message.into(),
        });
    }

    pub fn is_clean(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn count(&self) -> usize {
        self.messages.len()
    }
}

pub fn repair_scene(scene: &Scene) -> (Scene, RepairReport) {
    let mut repaired = scene.clone();
    let mut report = RepairReport::default();

    for (index, frame) in repaired.frames.iter_mut().enumerate() {
        repair_frame(frame, &format!("Scene/Frame[{index}]"), &mut report);
    }

    for (index, mesh) in repaired.loose_meshes.iter_mut().enumerate() {
        repair_mesh(mesh, &format!("Scene/LooseMesh[{index}]"), &mut report);
    }

    for (set_index, set) in repaired.animation_sets.iter_mut().enumerate() {
        for (anim_index, animation) in set.animations.iter_mut().enumerate() {
            repair_animation(
                animation,
                &format!("Scene/AnimationSet[{set_index}]/Animation[{anim_index}]"),
                &mut report,
            );
        }
    }

    (repaired, report)
}

fn repair_frame(frame: &mut Frame, path: &str, report: &mut RepairReport) {
    if frame.transform.is_none() {
        frame.transform = Some(identity_matrix());
        report.push(path.to_string(), "inserted identity FrameTransformMatrix");
    }

    for (index, mesh) in frame.meshes.iter_mut().enumerate() {
        repair_mesh(mesh, &format!("{path}/Mesh[{index}]"), report);
    }

    for (index, child) in frame.child_frames.iter_mut().enumerate() {
        repair_frame(child, &format!("{path}/Frame[{index}]"), report);
    }
}

fn repair_mesh(mesh: &mut Mesh, path: &str, report: &mut RepairReport) {
    if let Some(adjacency) = &mut mesh.face_adjacency {
        let expected = mesh.faces.len() * 3;
        if adjacency.indices.len() > expected {
            let old = adjacency.indices.len();
            adjacency.indices.truncate(expected);
            report.push(
                path.to_string(),
                format!("truncated FaceAdjacency from {} entries to expected {}", old, expected),
            );
        }
    }

    if let Some(face_wraps) = &mut mesh.mesh_face_wraps {
        if face_wraps.wraps.len() > mesh.faces.len() {
            let old = face_wraps.wraps.len();
            face_wraps.wraps.truncate(mesh.faces.len());
            report.push(
                path.to_string(),
                format!("truncated MeshFaceWraps from {} entries to mesh face count {}", old, mesh.faces.len()),
            );
        }
    }

    if let Some(duplication) = &mut mesh.vertex_duplication_indices {
        if duplication.indices.len() > mesh.vertices.len() {
            let old = duplication.indices.len();
            duplication.indices.truncate(mesh.vertices.len());
            report.push(
                path.to_string(),
                format!("truncated VertexDuplicationIndices from {} entries to vertex count {}", old, mesh.vertices.len()),
            );
        }
        if duplication.original_vertices > duplication.indices.len() as u32 {
            let old = duplication.original_vertices;
            duplication.original_vertices = duplication.indices.len() as u32;
            report.push(
                path.to_string(),
                format!("lowered VertexDuplicationIndices nOriginalVertices from {} to {}", old, duplication.original_vertices),
            );
        }
    }

    if let Some(texcoords) = &mut mesh.texcoords {
        if texcoords.len() > mesh.vertices.len() {
            let old = texcoords.len();
            texcoords.truncate(mesh.vertices.len());
            report.push(
                path.to_string(),
                format!("truncated MeshTextureCoords from {} entries to vertex count {}", old, mesh.vertices.len()),
            );
        }
    }

    if let Some(colors) = &mut mesh.vertex_colors {
        let before = colors.len();
        colors.retain(|color| (color.index as usize) < mesh.vertices.len());
        let removed = before.saturating_sub(colors.len());
        if removed != 0 {
            report.push(path.to_string(), format!("removed {} out-of-range MeshVertexColors entries", removed));
        }
    }

    if let Some(normals) = &mut mesh.normals {
        if normals.face_normals.len() > mesh.faces.len() {
            let old = normals.face_normals.len();
            normals.face_normals.truncate(mesh.faces.len());
            report.push(
                path.to_string(),
                format!("truncated MeshNormals face list from {} to mesh face count {}", old, mesh.faces.len()),
            );
        }
    }

    if let Some(material_list) = &mut mesh.material_list {
        repair_material_list(material_list, mesh.faces.len(), path, report);
    }

    if let Some(header) = &mut mesh.skin_mesh_header {
        let required = mesh.skin_weights.len() as u16;
        if header.bone_count < required {
            let old = header.bone_count;
            header.bone_count = required;
            report.push(
                path.to_string(),
                format!("raised XSkinMeshHeader bone_count from {} to {} to match SkinWeights objects", old, required),
            );
        }
    }

    for (index, skin) in mesh.skin_weights.iter_mut().enumerate() {
        repair_skin_weights(skin, mesh.vertices.len(), &format!("{path}/SkinWeights[{index}]"), report);
    }
}

fn repair_material_list(material_list: &mut MeshMaterialList, face_count: usize, path: &str, report: &mut RepairReport) {
    let payload_count = (material_list.materials.len() + material_list.material_references.len()) as u32;
    if material_list.material_count < payload_count {
        let old = material_list.material_count;
        material_list.material_count = payload_count;
        report.push(
            path.to_string(),
            format!("raised MeshMaterialList material_count from {} to payload count {}", old, payload_count),
        );
    }

    if material_list.face_indexes.len() > face_count {
        let old = material_list.face_indexes.len();
        material_list.face_indexes.truncate(face_count);
        report.push(
            path.to_string(),
            format!("truncated MeshMaterialList face indexes from {} to mesh face count {}", old, face_count),
        );
    }

    if material_list.material_count == 1 && material_list.face_indexes.len() < face_count {
        let missing = face_count - material_list.face_indexes.len();
        material_list.face_indexes.resize(face_count, 0);
        report.push(
            path.to_string(),
            format!("padded MeshMaterialList with {} default material index entries because material_count is 1", missing),
        );
    }

    if material_list.material_count == 1 {
        let mut rewrites = 0usize;
        for value in &mut material_list.face_indexes {
            if *value != 0 {
                *value = 0;
                rewrites += 1;
            }
        }
        if rewrites != 0 {
            report.push(
                path.to_string(),
                format!("rewrote {} MeshMaterialList face indexes to 0 because material_count is 1", rewrites),
            );
        }
    }
}

fn repair_skin_weights(skin: &mut SkinWeights, vertex_count: usize, path: &str, report: &mut RepairReport) {
    let aligned = skin.vertex_indices.len().min(skin.weights.len());
    if skin.vertex_indices.len() != skin.weights.len() {
        let old_indices = skin.vertex_indices.len();
        let old_weights = skin.weights.len();
        skin.vertex_indices.truncate(aligned);
        skin.weights.truncate(aligned);
        report.push(
            path.to_string(),
            format!("truncated SkinWeights streams to common length {} (indices {}, weights {})", aligned, old_indices, old_weights),
        );
    }

    let mut new_indices = Vec::with_capacity(skin.vertex_indices.len());
    let mut new_weights = Vec::with_capacity(skin.weights.len());
    let mut removed = 0usize;
    for (&vertex_index, &weight) in skin.vertex_indices.iter().zip(skin.weights.iter()) {
        if vertex_index as usize >= vertex_count {
            removed += 1;
            continue;
        }
        new_indices.push(vertex_index);
        new_weights.push(weight);
    }
    if removed != 0 {
        skin.vertex_indices = new_indices;
        skin.weights = new_weights;
        report.push(path.to_string(), format!("removed {} out-of-range SkinWeights entries", removed));
    }
}

fn repair_animation(animation: &mut Animation, path: &str, report: &mut RepairReport) {
    if animation.target_name.is_none() {
        if let Some(name) = &animation.name {
            animation.target_name = Some(name.clone());
            report.push(path.to_string(), "copied animation object name into target_name because no reference target was present");
        }
    }
}

fn identity_matrix() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]
}
