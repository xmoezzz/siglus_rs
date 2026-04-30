use crate::semantic::{Animation, CompressedAnimationSet, EffectInstance, Frame, Material, Mesh, PatchMesh, PMInfo, Scene};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationMessage {
    pub severity: ValidationSeverity,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub messages: Vec<ValidationMessage>,
}

impl ValidationReport {
    pub fn push(&mut self, severity: ValidationSeverity, path: impl Into<String>, message: impl Into<String>) {
        self.messages.push(ValidationMessage {
            severity,
            path: path.into(),
            message: message.into(),
        });
    }

    pub fn error(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.push(ValidationSeverity::Error, path, message);
    }

    pub fn warning(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.push(ValidationSeverity::Warning, path, message);
    }

    pub fn is_clean(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.messages.iter().filter(|m| m.severity == ValidationSeverity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.messages.iter().filter(|m| m.severity == ValidationSeverity::Warning).count()
    }
}

pub fn validate_scene(scene: &Scene) -> ValidationReport {
    let mut report = ValidationReport::default();
    let frame_names = collect_frame_names(scene);
    report_duplicate_frame_names(&frame_names, &mut report);
    let frame_name_set: HashSet<&str> = frame_names.keys().map(String::as_str).collect();

    for (index, frame) in scene.frames.iter().enumerate() {
        validate_frame(frame, &format!("Scene/Frame[{index}]"), &frame_name_set, &mut report);
    }

    for (index, mesh) in scene.loose_meshes.iter().enumerate() {
        validate_mesh(mesh, &format!("Scene/LooseMesh[{index}]"), &frame_name_set, &mut report);
    }

    for (index, patch_mesh) in scene.loose_patch_meshes.iter().enumerate() {
        validate_patch_mesh(patch_mesh, &format!("Scene/LoosePatchMesh[{index}]"), &mut report);
    }

    for (index, material) in scene.loose_materials.iter().enumerate() {
        validate_material(material, &format!("Scene/LooseMaterial[{index}]"), &mut report);
    }

    for (index, effect) in scene.loose_effect_instances.iter().enumerate() {
        validate_effect_instance(effect, &format!("Scene/LooseEffectInstance[{index}]"), &mut report);
    }

    for (index, compressed) in scene.compressed_animation_sets.iter().enumerate() {
        validate_compressed_animation_set(compressed, &format!("Scene/CompressedAnimationSet[{index}]"), &mut report);
    }

    for (set_index, set) in scene.animation_sets.iter().enumerate() {
        for (anim_index, animation) in set.animations.iter().enumerate() {
            validate_animation(
                animation,
                &format!("Scene/AnimationSet[{set_index}]/Animation[{anim_index}]"),
                &frame_name_set,
                &mut report,
            );
        }
    }

    report
}

fn collect_frame_names(scene: &Scene) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for frame in &scene.frames {
        collect_frame_names_from_frame(frame, &mut out);
    }
    out
}

fn collect_frame_names_from_frame(frame: &Frame, out: &mut HashMap<String, usize>) {
    if let Some(name) = &frame.name {
        *out.entry(name.clone()).or_insert(0) += 1;
    }
    for child in &frame.child_frames {
        collect_frame_names_from_frame(child, out);
    }
}

fn report_duplicate_frame_names(frame_names: &HashMap<String, usize>, report: &mut ValidationReport) {
    for (name, count) in frame_names {
        if *count > 1 {
            report.warning("Scene".to_string(), format!("frame name '{}' appears {} times", name, count));
        }
    }
}

fn validate_frame(frame: &Frame, path: &str, frame_name_set: &HashSet<&str>, report: &mut ValidationReport) {
    if frame.transform.is_none() {
        report.warning(path.to_string(), "frame has no FrameTransformMatrix");
    }

    for (index, mesh) in frame.meshes.iter().enumerate() {
        validate_mesh(mesh, &format!("{path}/Mesh[{index}]"), frame_name_set, report);
    }

    for (index, patch_mesh) in frame.patch_meshes.iter().enumerate() {
        validate_patch_mesh(patch_mesh, &format!("{path}/PatchMesh[{index}]"), report);
    }

    for (index, child) in frame.child_frames.iter().enumerate() {
        validate_frame(child, &format!("{path}/Frame[{index}]"), frame_name_set, report);
    }
}

fn validate_mesh(mesh: &Mesh, path: &str, frame_name_set: &HashSet<&str>, report: &mut ValidationReport) {
    if mesh.vertices.is_empty() {
        report.error(path.to_string(), "mesh has zero vertices");
    }

    if mesh.faces.is_empty() {
        report.warning(path.to_string(), "mesh has zero faces");
    }

    for (face_index, face) in mesh.faces.iter().enumerate() {
        if face.len() < 3 {
            report.error(
                format!("{path}/Face[{face_index}]"),
                format!("face has {} indices, expected at least 3", face.len()),
            );
        }
        for &vertex_index in face {
            if vertex_index as usize >= mesh.vertices.len() {
                report.error(
                    format!("{path}/Face[{face_index}]"),
                    format!("vertex index {} is out of range for {} vertices", vertex_index, mesh.vertices.len()),
                );
            }
        }
    }

    if let Some(adjacency) = &mesh.face_adjacency {
        let expected = mesh.faces.len() * 3;
        if adjacency.indices.len() != expected {
            report.warning(
                path.to_string(),
                format!("FaceAdjacency count {} does not match expected 3-per-face count {}", adjacency.indices.len(), expected),
            );
        }
    }

    if let Some(face_wraps) = &mesh.mesh_face_wraps {
        if face_wraps.wraps.len() != mesh.faces.len() {
            report.warning(
                path.to_string(),
                format!("MeshFaceWraps count {} does not match mesh face count {}", face_wraps.wraps.len(), mesh.faces.len()),
            );
        }
    }

    if let Some(duplication) = &mesh.vertex_duplication_indices {
        if duplication.indices.len() != mesh.vertices.len() {
            report.warning(
                path.to_string(),
                format!("VertexDuplicationIndices count {} does not match vertex count {}", duplication.indices.len(), mesh.vertices.len()),
            );
        }
        for (index, &original) in duplication.indices.iter().enumerate() {
            if original >= duplication.original_vertices {
                report.warning(
                    format!("{path}/VertexDuplicationIndices[{index}]"),
                    format!("duplicate source index {} is not smaller than nOriginalVertices {}", original, duplication.original_vertices),
                );
            }
        }
    }

    if let Some(fvf) = &mesh.fvf_data {
        if fvf.dwords.is_empty() {
            report.warning(path.to_string(), "FVFData is present but contains zero DWORD payload entries");
        }
    }

    if let Some(decl) = &mesh.decl_data {
        if decl.elements.is_empty() {
            report.warning(path.to_string(), "DeclData is present but contains zero VertexElement entries");
        }
        for (index, element) in decl.elements.iter().enumerate() {
            if element.usage_index > 15 {
                report.warning(
                    format!("{path}/DeclData/Element[{index}]"),
                    format!("usage_index {} exceeds the common Direct3D 9 declaration range 0..=15", element.usage_index),
                );
            }
        }
        if decl.dwords.is_empty() {
            report.warning(path.to_string(), "DeclData is present but contains zero DWORD payload entries");
        }
    }

    if let Some(texcoords) = &mesh.texcoords {
        if texcoords.len() != mesh.vertices.len() {
            report.warning(
                path.to_string(),
                format!("MeshTextureCoords count {} does not match vertex count {}", texcoords.len(), mesh.vertices.len()),
            );
        }
    }

    if let Some(colors) = &mesh.vertex_colors {
        for (color_index, color) in colors.iter().enumerate() {
            if color.index as usize >= mesh.vertices.len() {
                report.error(
                    format!("{path}/MeshVertexColors[{color_index}]"),
                    format!("vertex color index {} is out of range for {} vertices", color.index, mesh.vertices.len()),
                );
            }
        }
    }

    if let Some(normals) = &mesh.normals {
        if normals.face_normals.len() != mesh.faces.len() {
            report.warning(
                path.to_string(),
                format!("MeshNormals face count {} does not match mesh face count {}", normals.face_normals.len(), mesh.faces.len()),
            );
        }

        for (face_index, face) in normals.face_normals.iter().enumerate() {
            for &normal_index in face {
                if normal_index as usize >= normals.normals.len() {
                    report.error(
                        format!("{path}/MeshNormals/Face[{face_index}]"),
                        format!("normal index {} is out of range for {} normals", normal_index, normals.normals.len()),
                    );
                }
            }
        }
    }

    if let Some(material_list) = &mesh.material_list {
        if material_list.face_indexes.len() != mesh.faces.len() {
            report.warning(
                path.to_string(),
                format!("MeshMaterialList face-material count {} does not match mesh face count {}", material_list.face_indexes.len(), mesh.faces.len()),
            );
        }

        let material_slots = material_list.materials.len() + material_list.material_references.len();
        if material_slots > material_list.material_count as usize {
            report.warning(
                path.to_string(),
                format!("material payload count {} exceeds declared material_count {}", material_slots, material_list.material_count),
            );
        }

        for (face_index, &material_index) in material_list.face_indexes.iter().enumerate() {
            if material_index >= material_list.material_count {
                report.error(
                    format!("{path}/MeshMaterialList/Face[{face_index}]"),
                    format!("material index {} is out of range for declared material_count {}", material_index, material_list.material_count),
                );
            }
        }

        for (index, material) in material_list.materials.iter().enumerate() {
            validate_material(material, &format!("{path}/MeshMaterialList/Material[{index}]"), report);
        }
    }

    for (index, effect) in mesh.effect_instances.iter().enumerate() {
        validate_effect_instance(effect, &format!("{path}/EffectInstance[{index}]"), report);
    }

    if let Some(pm_info) = &mesh.pm_info {
        validate_pm_info(pm_info, &format!("{path}/PMInfo"), report);
    }

    if let Some(header) = &mesh.skin_mesh_header {
        if header.bone_count as usize != mesh.skin_weights.len() {
            report.warning(
                path.to_string(),
                format!("XSkinMeshHeader bone_count {} does not match SkinWeights object count {}", header.bone_count, mesh.skin_weights.len()),
            );
        }
    }

    for (skin_index, skin) in mesh.skin_weights.iter().enumerate() {
        if skin.vertex_indices.len() != skin.weights.len() {
            report.error(
                format!("{path}/SkinWeights[{skin_index}]"),
                format!("vertex index count {} does not match weight count {}", skin.vertex_indices.len(), skin.weights.len()),
            );
        }

        for &vertex_index in &skin.vertex_indices {
            if vertex_index as usize >= mesh.vertices.len() {
                report.error(
                    format!("{path}/SkinWeights[{skin_index}]"),
                    format!("skin vertex index {} is out of range for {} vertices", vertex_index, mesh.vertices.len()),
                );
            }
        }

        if !skin.transform_node_name.is_empty() && !frame_name_set.contains(skin.transform_node_name.as_str()) {
            report.warning(
                format!("{path}/SkinWeights[{skin_index}]"),
                format!("SkinWeights transform node '{}' does not resolve to any frame name", skin.transform_node_name),
            );
        }
    }
}


fn validate_material(material: &Material, path: &str, report: &mut ValidationReport) {
    for (index, effect) in material.effect_instances.iter().enumerate() {
        validate_effect_instance(effect, &format!("{path}/EffectInstance[{index}]"), report);
    }
}

fn validate_effect_instance(effect: &EffectInstance, path: &str, report: &mut ValidationReport) {
    if effect.effect_filename.is_empty() {
        report.warning(path.to_string(), "EffectInstance has an empty effect filename");
    }

    for (index, entry) in effect.strings.iter().enumerate() {
        if entry.param_name.is_empty() {
            report.warning(format!("{path}/EffectParamString[{index}]"), "effect string parameter name is empty");
        }
    }

    for (index, entry) in effect.dwords.iter().enumerate() {
        if entry.param_name.is_empty() {
            report.warning(format!("{path}/EffectParamDWord[{index}]"), "effect DWORD parameter name is empty");
        }
    }

    for (index, entry) in effect.floats.iter().enumerate() {
        if entry.param_name.is_empty() {
            report.warning(format!("{path}/EffectParamFloats[{index}]"), "effect float parameter name is empty");
        }
        if entry.values.is_empty() {
            report.warning(format!("{path}/EffectParamFloats[{index}]"), "effect float parameter contains zero values");
        }
    }
}

fn validate_patch_mesh(patch_mesh: &PatchMesh, path: &str, report: &mut ValidationReport) {
    if patch_mesh.vertices.is_empty() {
        report.warning(path.to_string(), "patch mesh has zero vertices");
    }

    if patch_mesh.patches.is_empty() {
        report.warning(path.to_string(), "patch mesh has zero patches");
    }

    for (patch_index, patch) in patch_mesh.patches.iter().enumerate() {
        if patch.control_indices.is_empty() {
            report.warning(format!("{path}/Patch[{patch_index}]"), "patch has zero control indices");
        }
        for &control_index in &patch.control_indices {
            if control_index as usize >= patch_mesh.vertices.len() {
                report.error(
                    format!("{path}/Patch[{patch_index}]"),
                    format!("patch control index {} is out of range for {} vertices", control_index, patch_mesh.vertices.len()),
                );
            }
        }
    }

    for (index, effect) in patch_mesh.effect_instances.iter().enumerate() {
        validate_effect_instance(effect, &format!("{path}/EffectInstance[{index}]"), report);
    }

    if let Some(pm_info) = &patch_mesh.pm_info {
        validate_pm_info(pm_info, &format!("{path}/PMInfo"), report);
    }
}

fn validate_pm_info(pm_info: &PMInfo, path: &str, report: &mut ValidationReport) {
    if pm_info.min_logical_vertices > pm_info.max_logical_vertices {
        report.warning(
            path.to_string(),
            format!(
                "PMInfo min logical vertices {} exceeds max logical vertices {}",
                pm_info.min_logical_vertices, pm_info.max_logical_vertices
            ),
        );
    }

    for (index, range) in pm_info.attribute_ranges.iter().enumerate() {
        if range.faces_min > range.faces_max {
            report.warning(
                format!("{path}/PMAttributeRange[{index}]"),
                format!("PMAttributeRange faces_min {} exceeds faces_max {}", range.faces_min, range.faces_max),
            );
        }
        if range.vertices_min > range.vertices_max {
            report.warning(
                format!("{path}/PMAttributeRange[{index}]"),
                format!(
                    "PMAttributeRange vertices_min {} exceeds vertices_max {}",
                    range.vertices_min, range.vertices_max
                ),
            );
        }
    }
}

fn validate_compressed_animation_set(
    compressed: &CompressedAnimationSet,
    path: &str,
    report: &mut ValidationReport,
) {
    if compressed.buffer_length as usize != compressed.compressed_data.len() {
        report.error(
            path.to_string(),
            format!(
                "CompressedAnimationSet buffer_length {} does not match DWORD payload count {}",
                compressed.buffer_length,
                compressed.compressed_data.len()
            ),
        );
    }

    let expected_words = (compressed.compressed_block_size + 3) / 4;
    if compressed.buffer_length != expected_words {
        report.warning(
            path.to_string(),
            format!(
                "CompressedAnimationSet buffer_length {} does not match ceil(CompressedBlockSize/4) {}",
                compressed.buffer_length, expected_words
            ),
        );
    }
}

fn validate_animation(animation: &Animation, path: &str, frame_name_set: &HashSet<&str>, report: &mut ValidationReport) {
    if animation.target_name.is_none() && animation.target_uuid.is_none() {
        report.warning(path.to_string(), "animation has no reference target");
    }

    if let Some(target_name) = &animation.target_name {
        if !frame_name_set.contains(target_name.as_str()) {
            report.warning(path.to_string(), format!("animation target '{}' does not resolve to any frame name", target_name));
        }
    }

    if animation.keys.is_empty() {
        report.warning(path.to_string(), "animation contains no AnimationKey blocks");
    }

    for (block_index, block) in animation.keys.iter().enumerate() {
        if !matches!(block.key_type, 0 | 1 | 2 | 3 | 4) {
            report.warning(format!("{path}/AnimationKey[{block_index}]"), format!("unknown animation key type {}", block.key_type));
        }

        if block.keys.is_empty() {
            report.warning(format!("{path}/AnimationKey[{block_index}]"), "animation key block contains no keyed samples");
        }

        let mut previous_time = None;
        for (sample_index, key) in block.keys.iter().enumerate() {
            if key.values.is_empty() {
                report.warning(format!("{path}/AnimationKey[{block_index}]/Key[{sample_index}]"), "timed key has zero values");
            }

            if let Some(prev) = previous_time {
                if key.time < prev {
                    report.warning(
                        format!("{path}/AnimationKey[{block_index}]/Key[{sample_index}]"),
                        format!("animation key time {} is earlier than previous sample {}", key.time, prev),
                    );
                }
            }
            previous_time = Some(key.time);
        }
    }
}
