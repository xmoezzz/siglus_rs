#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTemplateInfo {
    pub name: &'static str,
    pub guid: &'static str,
}

pub const OFFICIAL_TEMPLATES: &[BuiltinTemplateInfo] = &[
    BuiltinTemplateInfo { name: "Animation", guid: "3D82AB4F-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "AnimationKey", guid: "10DD46A8-775B-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "AnimationOptions", guid: "E2BF56C0-840F-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "AnimationSet", guid: "3D82AB50-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "AnimTicksPerSecond", guid: "9E415A43-7BA6-4A73-8743-B73D47E88476" },
    BuiltinTemplateInfo { name: "Boolean", guid: "537DA6A0-CA37-11D0-941C-0080C80CFA7B" },
    BuiltinTemplateInfo { name: "Boolean2d", guid: "4885AE63-78E8-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "ColorRGB", guid: "D3E16E81-7835-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "ColorRGBA", guid: "35FF44E0-6C7C-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "CompressedAnimationSet", guid: "7F9B00B3-F125-4890-876E-1C42BF697C4D" },
    BuiltinTemplateInfo { name: "Coords2d", guid: "F6F23F44-7686-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "DeclData", guid: "BF22E553-292C-4781-9FEA-62BD554BDD93" },
    BuiltinTemplateInfo { name: "EffectDWord", guid: "622C0ED0-956E-4DA9-908A-2AF94F3CE716" },
    BuiltinTemplateInfo { name: "EffectFloats", guid: "F1CFE2B3-0DE3-4E28-AFA1-155A750A282D" },
    BuiltinTemplateInfo { name: "EffectInstance", guid: "E331F7E4-0559-4CC2-8E99-1CEC1657928F" },
    BuiltinTemplateInfo { name: "EffectParamDWord", guid: "E13963BC-AE51-4C5D-B00F-CFA3A9D97CE5" },
    BuiltinTemplateInfo { name: "EffectParamFloats", guid: "3014B9A0-62F5-478C-9B86-E4AC9F4E418B" },
    BuiltinTemplateInfo { name: "EffectParamString", guid: "1DBC4C88-94C1-46EE-9076-2C28818C9481" },
    BuiltinTemplateInfo { name: "EffectString", guid: "D55B097E-BDB6-4C52-B03D-6051C89D0E42" },
    BuiltinTemplateInfo { name: "FaceAdjacency", guid: "A64C844A-E282-4756-8B80-250CDE04398C" },
    BuiltinTemplateInfo { name: "FloatKeys", guid: "10DD46A9-775B-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "Frame", guid: "3D82AB46-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "FrameTransformMatrix", guid: "F6F23F41-7686-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "FVFData", guid: "B6E70A0E-8EF9-4E83-94AD-ECC8B0C04897" },
    BuiltinTemplateInfo { name: "Guid", guid: "A42790E0-7810-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "IndexedColor", guid: "1630B820-7842-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "Material", guid: "3D82AB4D-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "MaterialWrap", guid: "4885AE60-78E8-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "Matrix4x4", guid: "F6F23F45-7686-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "Mesh", guid: "3D82AB44-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "MeshFace", guid: "3D82AB5F-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "MeshFaceWraps", guid: "ED1EC5C0-C0A8-11D0-941C-0080C80CFA7B" },
    BuiltinTemplateInfo { name: "MeshMaterialList", guid: "F6F23F42-7686-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "MeshNormals", guid: "F6F23F43-7686-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "MeshTextureCoords", guid: "F6F23F40-7686-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "MeshVertexColors", guid: "1630B821-7842-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "Patch", guid: "A3EB5D44-FC22-429D-9AFB-3221CB9719A6" },
    BuiltinTemplateInfo { name: "PatchMesh", guid: "D02C95CC-EDBA-4305-9B5D-1820D7704BBF" },
    BuiltinTemplateInfo { name: "PatchMesh9", guid: "B9EC94E1-B9A6-4251-BA18-94893F02C0EA" },
    BuiltinTemplateInfo { name: "PMAttributeRange", guid: "917E0427-C61E-4A14-9C64-AFE65F9E9844" },
    BuiltinTemplateInfo { name: "PMInfo", guid: "B6C3E656-EC8B-4B92-9B62-681659522947" },
    BuiltinTemplateInfo { name: "PMVSplitRecord", guid: "574CCC14-F0B3-4333-822D-93E8A8A08E4C" },
    BuiltinTemplateInfo { name: "SkinWeights", guid: "6F0D123B-BAD2-4167-A0D0-80224F25FABB" },
    BuiltinTemplateInfo { name: "TextureFilename", guid: "A42790E1-7810-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "TimedFloatKeys", guid: "F406B180-7B3B-11CF-8F52-0040333594A3" },
    BuiltinTemplateInfo { name: "Vector", guid: "3D82AB5E-62DA-11CF-AB39-0020AF71E433" },
    BuiltinTemplateInfo { name: "VertexDuplicationIndices", guid: "B8D65549-D7C9-4995-89CF-53A9A8B031E3" },
    BuiltinTemplateInfo { name: "VertexElement", guid: "F752461C-1E23-48F6-B9F8-8350850F336F" },
    BuiltinTemplateInfo { name: "XSkinMeshHeader", guid: "3CF169CE-FF7C-44AB-93C0-F78F62D172E2" },
];

pub const OFFICIAL_TEMPLATE_NAMES: &[&str] = &[
    "Animation",
    "AnimationKey",
    "AnimationOptions",
    "AnimationSet",
    "AnimTicksPerSecond",
    "Boolean",
    "Boolean2d",
    "ColorRGB",
    "ColorRGBA",
    "CompressedAnimationSet",
    "Coords2d",
    "DeclData",
    "EffectDWord",
    "EffectFloats",
    "EffectInstance",
    "EffectParamDWord",
    "EffectParamFloats",
    "EffectParamString",
    "EffectString",
    "FaceAdjacency",
    "FloatKeys",
    "Frame",
    "FrameTransformMatrix",
    "FVFData",
    "Guid",
    "IndexedColor",
    "Material",
    "MaterialWrap",
    "Matrix4x4",
    "Mesh",
    "MeshFace",
    "MeshFaceWraps",
    "MeshMaterialList",
    "MeshNormals",
    "MeshTextureCoords",
    "MeshVertexColors",
    "Patch",
    "PatchMesh",
    "PatchMesh9",
    "PMAttributeRange",
    "PMInfo",
    "PMVSplitRecord",
    "SkinWeights",
    "TextureFilename",
    "TimedFloatKeys",
    "Vector",
    "VertexDuplicationIndices",
    "VertexElement",
    "XSkinMeshHeader",
];

pub const STRUCTURED_TEMPLATE_NAMES: &[&str] = OFFICIAL_TEMPLATE_NAMES;

pub fn is_official_template_name(name: &str) -> bool {
    OFFICIAL_TEMPLATE_NAMES
        .iter()
        .any(|official| official.eq_ignore_ascii_case(name))
}

pub fn is_structured_template_name(name: &str) -> bool {
    STRUCTURED_TEMPLATE_NAMES
        .iter()
        .any(|official| official.eq_ignore_ascii_case(name))
}

pub fn unsupported_official_templates() -> Vec<&'static str> {
    OFFICIAL_TEMPLATE_NAMES
        .iter()
        .copied()
        .filter(|name| !is_structured_template_name(name))
        .collect()
}

pub fn official_template_info(name: &str) -> Option<&'static BuiltinTemplateInfo> {
    OFFICIAL_TEMPLATES
        .iter()
        .find(|info| info.name.eq_ignore_ascii_case(name))
}

pub fn official_template_guid(name: &str) -> Option<&'static str> {
    official_template_info(name).map(|info| info.guid)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateGuidMismatch {
    pub name: String,
    pub expected_guid: &'static str,
    pub actual_guid: String,
}

pub fn collect_template_guid_mismatches<'a, I>(templates: I) -> Vec<TemplateGuidMismatch>
where
    I: IntoIterator<Item = &'a crate::model::XTemplateDef>,
{
    let mut mismatches = Vec::new();
    for template in templates {
        if let Some(expected_guid) = official_template_guid(&template.name) {
            if !template.uuid.as_str().eq_ignore_ascii_case(expected_guid) {
                mismatches.push(TemplateGuidMismatch {
                    name: template.name.clone(),
                    expected_guid,
                    actual_guid: template.uuid.as_str().to_string(),
                });
            }
        }
    }
    mismatches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Guid, XTemplateDef};

    #[test]
    fn official_catalog_is_complete_for_names() {
        assert_eq!(OFFICIAL_TEMPLATES.len(), OFFICIAL_TEMPLATE_NAMES.len());
        for name in OFFICIAL_TEMPLATE_NAMES {
            let info = official_template_info(name).expect("missing template info");
            assert_eq!(info.name, *name);
            assert_eq!(official_template_guid(name), Some(info.guid));
        }
    }

    #[test]
    fn collects_guid_mismatches() {
        let defs = vec![
            XTemplateDef {
                name: "Mesh".to_string(),
                uuid: Guid::parse("<3D82AB44-62DA-11CF-AB39-0020AF71E433>").unwrap(),
                members: Vec::new(),
                restrictions: Vec::new(),
            },
            XTemplateDef {
                name: "Frame".to_string(),
                uuid: Guid::parse("<AAAAAAAA-62DA-11CF-AB39-0020AF71E433>").unwrap(),
                members: Vec::new(),
                restrictions: Vec::new(),
            },
        ];
        let mismatches = collect_template_guid_mismatches(&defs);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].name, "Frame");
        assert_eq!(mismatches[0].expected_guid, "3D82AB46-62DA-11CF-AB39-0020AF71E433");
    }
}
