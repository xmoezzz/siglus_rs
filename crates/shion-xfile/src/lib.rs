pub mod binary;
pub mod builtin_templates;
pub mod compression;
pub mod error;
pub mod guid;
pub mod header;
pub mod model;
pub mod semantic;
pub mod repair;
pub mod text_lexer;
pub mod text_parser;
pub mod validation;

pub use binary::{parse_binary_file, tokenize_binary, BinaryTokenKind, BinaryTokenRecord};
pub use compression::decompress_mszip_payload;
pub use error::{Error, Result};
pub use guid::Guid;
pub use header::{FloatSize, FormatKind, XFileHeader};
pub use model::{
    PrimitiveValue, ReferenceTarget, TopLevelItem, XDataObject, XFile, XObjectElement, XTemplateDef,
    XTemplateMember, XTemplateRestriction,
};
pub use repair::{repair_scene, RepairMessage, RepairReport};
pub use semantic::{
    Animation, AnimationKeyBlock, Boolean, Boolean2d, ColorRGB, ColorRGBA, CompressedAnimationSet,
    Coords2d, DeclData, EffectDWord, EffectFloats, EffectInstance, EffectParamDWord,
    EffectParamFloats, EffectParamString, EffectString, FaceAdjacency, FloatKeys, Frame,
    FvfData, GuidValue, IndexedColor, Material, MaterialWrap, Matrix4x4, Mesh, MeshFace,
    MeshFaceWraps, PMAttributeRange, PMInfo, PMVSplitRecord, Patch, PatchMesh, Scene, SkinWeights,
    TimedFloatKeys, Vector, VertexDuplicationIndices, VertexElementDecl,
};
pub use builtin_templates::{
    collect_template_guid_mismatches, official_template_guid, official_template_info,
    BuiltinTemplateInfo, TemplateGuidMismatch,
};
pub use validation::{validate_scene, ValidationMessage, ValidationReport, ValidationSeverity};

pub fn parse_x(bytes: &[u8]) -> Result<XFile> {
    let (header, header_len) = XFileHeader::parse(bytes)?;
    let body = &bytes[header_len..];
    match header.format {
        FormatKind::Text => text_parser::parse_text_file(header, body),
        FormatKind::Binary => binary::parse_binary_file(header, body),
        FormatKind::TextMsZip => {
            let inflated = compression::decompress_mszip_payload(body)?;
            text_parser::parse_text_file(header, &inflated)
        }
        FormatKind::BinaryMsZip => {
            let inflated = compression::decompress_mszip_payload(body)?;
            binary::parse_binary_file(header, &inflated)
        }
        FormatKind::Unknown(raw) => Err(Error::Unsupported(format!(
            "unsupported .x format tag: {:?}",
            raw
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use std::io::Write;

    const SAMPLE: &str = "xof 0303txt 0032
\
        template FrameTransformMatrix { <F6F23F41-7686-11cf-8F52-0040333594A3> array FLOAT matrix[16]; }
\
        Frame Root {
\
            FrameTransformMatrix {
\
                1.0;0.0;0.0;0.0;,
\
                0.0;1.0;0.0;0.0;,
\
                0.0;0.0;1.0;0.0;,
\
                0.0;0.0;0.0;1.0;;
\
            }
\
            Mesh Mesh0 {
\
                3;
\
                0.0;0.0;0.0;,
\
                1.0;0.0;0.0;,
\
                0.0;1.0;0.0;;
\
                1;
\
                3;0,1,2;;
\
            }
\
        }
";

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_f32(out: &mut Vec<u8>, v: f32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_name(out: &mut Vec<u8>, s: &str) {
        push_u16(out, 1);
        push_u32(out, s.len() as u32);
        out.extend_from_slice(s.as_bytes());
    }

    fn binary_mesh_sample() -> Vec<u8> {
        let mut sample = Vec::new();
        sample.extend_from_slice(b"xof 0303bin 0032");
        push_name(&mut sample, "Mesh");
        push_name(&mut sample, "M");
        push_u16(&mut sample, 10);
        push_u16(&mut sample, 3);
        push_u32(&mut sample, 3);
        push_u16(&mut sample, 7);
        push_u32(&mut sample, 9);
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            push_f32(&mut sample, v);
        }
        push_u16(&mut sample, 3);
        push_u32(&mut sample, 1);
        push_u16(&mut sample, 6);
        push_u32(&mut sample, 4);
        for v in [3u32, 0, 1, 2] {
            push_u32(&mut sample, v);
        }
        push_u16(&mut sample, 11);
        sample
    }

    fn wrap_mszip(format_tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut out = Vec::new();
        out.extend_from_slice(b"xof 0303");
        out.extend_from_slice(format_tag);
        out.extend_from_slice(b"0032");
        out.extend_from_slice(&[0u8; 6]);
        out.extend_from_slice(&(compressed.len() as u16).to_le_bytes());
        out.extend_from_slice(&compression::MSZIP_MAGIC.to_le_bytes());
        out.extend_from_slice(&compressed);
        out
    }

    #[test]
    fn parses_header() {
        let (header, consumed) = XFileHeader::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(consumed, 16);
        assert_eq!(header.major, 3);
        assert_eq!(header.minor, 3);
        assert_eq!(header.float_size, FloatSize::F32);
    }

    #[test]
    fn parses_text_scene() {
        let file = parse_x(SAMPLE.as_bytes()).unwrap();
        assert_eq!(file.templates.len(), 1);
        let scene = Scene::from_xfile(&file).unwrap();
        assert_eq!(scene.frames.len(), 1);
        assert_eq!(scene.frames[0].meshes.len(), 1);
        assert_eq!(scene.frames[0].meshes[0].vertices.len(), 3);
        assert_eq!(scene.frames[0].meshes[0].faces[0], vec![0, 1, 2]);
    }

    #[test]
    fn validates_scene_diagnostics() {
        let sample = b"xof 0303txt 0032
Frame Root {
  FrameTransformMatrix { 1.0;0.0;0.0;0.0;,0.0;1.0;0.0;0.0;,0.0;0.0;1.0;0.0;,0.0;0.0;0.0;1.0;; }
  Mesh Mesh0 {
    3;
    0.0;0.0;0.0;,
    1.0;0.0;0.0;,
    0.0;1.0;0.0;;
    1;
    3;0,1,2;;
    MeshTextureCoords { 1; 0.0;0.0;; }
    MeshVertexColors { 1; 9; 1.0;1.0;1.0;1.0;; }
  }
}
";
        let file = parse_x(sample).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        let report = validate_scene(&scene);
        assert!(report.warning_count() >= 1);
        assert!(report.error_count() >= 1);
    }

    #[test]
    fn parses_binary_via_dispatch() {
        let file = parse_x(&binary_mesh_sample()).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        assert_eq!(scene.loose_meshes.len(), 1);
        assert_eq!(scene.loose_meshes[0].faces[0], vec![0, 1, 2]);
    }

    #[test]
    fn parses_tzip_scene() {
        let compressed = wrap_mszip(b"tzip", &SAMPLE.as_bytes()[16..]);
        let file = parse_x(&compressed).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        assert_eq!(file.header.format, FormatKind::TextMsZip);
        assert_eq!(scene.frames.len(), 1);
        assert_eq!(scene.frames[0].meshes[0].faces[0], vec![0, 1, 2]);
    }


    #[test]
    fn repairs_common_wild_export_damage() {
        let sample = b"xof 0303txt 0032
Frame Root {
  Mesh Mesh0 {
    3;
    0.0;0.0;0.0;,
    1.0;0.0;0.0;,
    0.0;1.0;0.0;;
    1;
    3;0,1,2;;
    MeshMaterialList {
      1;
      0;
      Material Mat0 {
        1.0;1.0;1.0;1.0;;
        0.0;
        0.0;0.0;0.0;;
        0.0;0.0;0.0;;
      }
    }
  }
}
";
        let file = parse_x(sample).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        let (repaired, report) = repair_scene(&scene);
        assert!(!report.is_clean());
        assert_eq!(repaired.frames[0].transform.unwrap()[0], 1.0);
        assert_eq!(repaired.frames[0].meshes[0].material_list.as_ref().unwrap().face_indexes, vec![0]);
    }

    #[test]
    fn parses_bzip_scene() {
        let raw_binary = binary_mesh_sample();
        let compressed = wrap_mszip(b"bzip", &raw_binary[16..]);
        let file = parse_x(&compressed).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        assert_eq!(file.header.format, FormatKind::BinaryMsZip);
        assert_eq!(scene.loose_meshes.len(), 1);
        assert_eq!(scene.loose_meshes[0].faces[0], vec![0, 1, 2]);
    }
}
