//! One Eluna player over the model definitions from all CREATE_EMOTE sources.
//!
//! Eluna 0.1's SDK constructor only accepts one serialized PSB. Its public
//! schema and player APIs also work with a combined, already parsed archive.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use eluna::{
    ElunaPlayer, EmoteLoadOptions, EmoteModelSchema, EmotePlayerControl, EmoteStaticScene, PsbFile,
    PsbValue, TimelinePlayMode, collect_emote_runtime_pipeline, collect_emote_timelines,
    collect_emote_variables,
};

#[derive(Debug, Clone)]
struct Model {
    data: Vec<u8>,
    psb: PsbFile,
    schema: EmoteModelSchema,
    motion: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Player {
    model: Arc<Model>,
    pub(super) inner: ElunaPlayer,
}

impl Player {
    pub(super) fn from_sources(sources: &[&[u8]], key: Option<u32>) -> Result<Self> {
        let mut options = EmoteLoadOptions::default();
        if let Some(key) = key {
            options = options.with_emote_key(key);
        }
        let mut archives = sources.iter().enumerate().map(|(index, bytes)| {
            PsbFile::parse_normalized(bytes, &options.normalize)
                .with_context(|| format!("failed to parse Emote PSB source {}", index + 1))
        });
        let Some(first) = archives.next() else {
            bail!("CREATE_EMOTE requires at least one PSB source");
        };
        let (mut data, mut psb) = first?;
        for archive in archives {
            let (bytes, mut next) = archive?;
            append_archive(&mut data, &mut psb, &bytes, &mut next)?;
        }
        let schema = EmoteModelSchema::from_psb(&psb)?;
        let motion = entry_motion(&psb, &schema)?;
        let variables = collect_emote_variables(&psb);
        let initial_values = variables
            .iter()
            .map(|variable| (variable.name.clone(), variable.default_value))
            .collect::<BTreeMap<_, _>>();
        let scene = match motion.as_deref() {
            Some(motion) => schema.build_motion_scene_at_with_resources_and_variables(
                &psb,
                &data,
                motion,
                0.0,
                &initial_values,
            )?,
            None => schema.build_static_scene(&psb)?,
        };
        // CreatePlayer does not autoplay; the script starts shared timelines.
        let inner = ElunaPlayer::from_scene_variables_timelines_runtime(
            scene,
            variables,
            collect_emote_timelines(&psb),
            collect_emote_runtime_pipeline(&psb),
        );
        let mut player = Self {
            model: Arc::new(Model {
                data,
                psb,
                schema,
                motion,
            }),
            inner,
        };
        player.rebuild_scene(0.0)?;
        Ok(player)
    }

    pub(super) fn schema(&self) -> &EmoteModelSchema {
        &self.model.schema
    }

    pub(super) fn texture_bytes(&self, index: u32) -> Option<&[u8]> {
        self.model
            .psb
            .resource_bytes(&self.model.data, index as usize)
    }

    pub(super) fn scene(&self) -> &EmoteStaticScene {
        self.inner.scene()
    }

    pub(super) fn progress_ticks(&mut self, ticks: f32) -> Result<()> {
        self.inner.progress_ticks_without_physics(ticks);
        self.rebuild_scene(ticks)
    }

    pub(super) fn play_timeline(&mut self, name: &str, mode: TimelinePlayMode) -> Result<()> {
        if !self.inner.timelines().contains_key(name) {
            bail!("missing Emote timeline {name:?}");
        }
        self.inner.play_timeline(name, mode);
        self.rebuild_scene(0.0)
    }

    pub(super) fn rebuild_scene(&mut self, physics_ticks: f32) -> Result<()> {
        let model = &self.model;
        let Some(motion) = model.motion.as_deref() else {
            return Ok(());
        };
        // Both passes must see the same previous frame, as in EmoteRuntime.
        // Otherwise HOLD and nested-motion dt=2 advance twice during physics.
        let build = |player: &ElunaPlayer, previous: &EmoteStaticScene| {
            model
                .schema
                .build_motion_scene_at_with_resources_variables_and_previous_scene(
                    &model.psb,
                    &model.data,
                    motion,
                    player.elapsed_ticks(),
                    &player.evaluated_variable_values(),
                    previous,
                )
        };
        let scene = build(&self.inner, self.inner.scene())?;
        let previous = self.inner.swap_scene(scene);
        if physics_ticks > 0.0 && self.inner.is_physics_enabled() {
            self.inner.evaluate_physics_for_current_scene(physics_ticks);
            self.inner.replace_scene(build(&self.inner, &previous)?);
        }
        Ok(())
    }
}

fn append_archive(
    data: &mut Vec<u8>,
    psb: &mut PsbFile,
    bytes: &[u8],
    next: &mut PsbFile,
) -> Result<()> {
    let resource_base = u32::try_from(psb.resources.len())?;
    let extra_base = u32::try_from(psb.extra_resources.len())?;
    let easing_base = psb
        .root
        .field("easing")
        .and_then(PsbValue::as_list)
        .map_or(0, <[_]>::len);
    rename_colliding_sources(&psb.root, &mut next.root);
    rebase_texture_indices(&mut next.root, resource_base)?;
    rebase_references(&mut next.root, resource_base, extra_base, easing_base)?;
    for range in next.resources.iter_mut().chain(&mut next.extra_resources) {
        range.offset = range
            .offset
            .checked_add(data.len() as u64)
            .context("Emote resource offset overflow")?;
    }
    merge_root(
        &mut psb.root,
        std::mem::replace(&mut next.root, PsbValue::Null),
    )?;
    data.extend_from_slice(bytes);
    psb.resources.append(&mut next.resources);
    psb.extra_resources.append(&mut next.extra_resources);
    // PsbValue already owns resolved names and strings. Header offsets and
    // parser tables are not used to evaluate the combined model.
    Ok(())
}

fn entry_motion(psb: &PsbFile, schema: &EmoteModelSchema) -> Result<Option<String>> {
    // The common timeline PSB selects the wrapper motion. The SDK's fallback
    // chooses the first motion, which can instead be the body's inner rig.
    if let Some(motion) = psb
        .root
        .field("metadata")
        .and_then(|metadata| metadata.field("base"))
        .and_then(|base| base.field_str("motion"))
        .filter(|motion| !motion.is_empty())
    {
        return Ok(Some(motion.to_owned()));
    }
    Ok(schema.default_motion_name(psb)?)
}

/// Each PSB of a CREATE_EMOTE set has its own `source` table: a layer's
/// `src` names a source of the file it comes from. Body and head files
/// both commonly name theirs `tex`; merged by name, the head's table would
/// replace the body's and every body layer would take its icon from the
/// head's texture. Rename the incoming file's colliding sources, and its
/// references to them, before the merge.
fn rename_colliding_sources(root: &PsbValue, next: &mut PsbValue) {
    let Some(PsbValue::Object(existing)) = root.field("source") else {
        return;
    };
    let PsbValue::Object(fields) = next else {
        return;
    };
    let Some((_, PsbValue::Object(sources))) = fields.iter_mut().find(|(name, _)| name == "source")
    else {
        return;
    };
    let mut renamed = HashMap::new();
    for index in 0..sources.len() {
        let name = sources[index].0.clone();
        if !existing.iter().any(|(key, _)| key == &name) {
            continue;
        }
        let unique = (1..)
            .map(|n| format!("{name}@{n}"))
            .find(|candidate| {
                !existing
                    .iter()
                    .chain(sources.iter())
                    .any(|(key, _)| key == candidate)
            })
            .expect("unbounded suffixes");
        sources[index].0 = unique.clone();
        renamed.insert(name, unique);
    }
    if !renamed.is_empty() {
        rename_source_references(next, &renamed);
    }
}

fn rename_source_references(value: &mut PsbValue, renamed: &HashMap<String, String>) {
    match value {
        PsbValue::List(values) => {
            for value in values {
                rename_source_references(value, renamed);
            }
        }
        PsbValue::Object(fields) => {
            for (name, value) in fields {
                if name == "src"
                    && let PsbValue::String(src) = value
                    && let Some(new) = renamed.get(src.as_str())
                {
                    *src = new.clone();
                    continue;
                }
                rename_source_references(value, renamed);
            }
        }
        _ => {}
    }
}

fn rebase_texture_indices(root: &mut PsbValue, base: u32) -> Result<()> {
    let PsbValue::Object(fields) = root else {
        return Ok(());
    };
    let Some((_, PsbValue::Object(sources))) = fields.iter_mut().find(|(name, _)| name == "source")
    else {
        return Ok(());
    };
    for (_, source) in sources {
        let PsbValue::Object(fields) = source else {
            continue;
        };
        let Some((_, PsbValue::Object(texture))) =
            fields.iter_mut().find(|(name, _)| name == "texture")
        else {
            continue;
        };
        // Eluna also accepts integer resource indices in these texture fields.
        // Typed Resource values are rebased by the recursive walk below.
        for (name, value) in texture {
            if matches!(name.as_str(), "pixel" | "data" | "resource")
                && let PsbValue::Int(index) = value
            {
                *index = u32::try_from(*index)?
                    .checked_add(base)
                    .context("Emote texture resource index overflow")?
                    as i64;
            }
        }
    }
    Ok(())
}

fn rebase_references(
    value: &mut PsbValue,
    resources: u32,
    extra: u32,
    easing: usize,
) -> Result<()> {
    match value {
        PsbValue::Resource(index) => {
            *index = index
                .checked_add(resources)
                .context("Emote resource index overflow")?;
        }
        PsbValue::ExtraResource(index) => {
            *index = index
                .checked_add(extra)
                .context("Emote extra resource index overflow")?;
        }
        PsbValue::List(values) => {
            for value in values {
                rebase_references(value, resources, extra, easing)?;
            }
        }
        PsbValue::Object(fields) => {
            for (name, value) in fields {
                if matches!(name.as_str(), "ccc" | "acc" | "zcc" | "scc" | "occ" | "wcc")
                    && let PsbValue::Int(index) = value
                {
                    // Negative indices mean no curve; inline curves need
                    // no adjustment. The remaining integers index easing.
                    if *index >= 0 {
                        *index = index
                            .checked_add(i64::try_from(easing)?)
                            .context("Emote easing index overflow")?;
                    }
                }
                rebase_references(value, resources, extra, easing)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Overlay named definitions, keeping motions supplied by earlier files.
/// Do not concatenate frame/parameter lists: their indices are motion-local.
fn merge_root(root: &mut PsbValue, next: PsbValue) -> Result<()> {
    let (PsbValue::Object(root), PsbValue::Object(next)) = (root, next) else {
        bail!("Emote PSB root must be an object");
    };
    for (name, value) in next {
        match name.as_str() {
            // object name -> object fields -> motion name. Each motion value
            // is a complete definition, including its local frame indices.
            "object" => merge_dictionary(entry(root, name), value, 2),
            "source" => merge_dictionary(entry(root, name), value, 0),
            "easing" => {
                let target = entry(root, name);
                if let (PsbValue::List(curves), PsbValue::List(next)) = (&mut *target, &value) {
                    curves.extend_from_slice(next);
                } else if !matches!(value, PsbValue::Null) {
                    *target = value;
                }
            }
            _ if !matches!(value, PsbValue::Null) => *entry(root, name) = value,
            _ => {}
        }
    }
    Ok(())
}

fn entry(fields: &mut Vec<(String, PsbValue)>, name: String) -> &mut PsbValue {
    let index = fields
        .iter()
        .position(|(key, _)| key == &name)
        .unwrap_or_else(|| {
            fields.push((name, PsbValue::Null));
            fields.len() - 1
        });
    &mut fields[index].1
}

fn merge_dictionary(target: &mut PsbValue, incoming: PsbValue, depth: usize) {
    if matches!(incoming, PsbValue::Null) {
        return;
    }
    if let (PsbValue::Object(fields), PsbValue::Object(next)) = (&mut *target, &incoming) {
        for (name, value) in next {
            let target = entry(fields, name.clone());
            if depth > 0 {
                merge_dictionary(target, value.clone(), depth - 1);
            } else {
                *target = value.clone();
            }
        }
    } else {
        *target = incoming;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eluna::{PsbHeader, PsbResourceRange};

    fn object(fields: &[(&str, PsbValue)]) -> PsbValue {
        PsbValue::Object(
            fields
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
        )
    }

    fn string(value: &str) -> PsbValue {
        PsbValue::String(value.into())
    }

    fn archive(root: PsbValue) -> PsbFile {
        let mut header = [0u8; 40];
        header[..6].copy_from_slice(b"PSB\0\x02\0");
        PsbFile {
            header: PsbHeader::read(&header).unwrap(),
            version: 2,
            encrypted: false,
            checksum: None,
            names: vec![],
            strings: vec![],
            root,
            resources: vec![PsbResourceRange {
                offset: 1,
                length: 2,
            }],
            extra_resources: vec![PsbResourceRange {
                offset: 3,
                length: 1,
            }],
        }
    }

    #[test]
    fn colliding_sources_keep_their_own_file() {
        // Body and head both name their source `tex`; each file's layers
        // must keep drawing from that file's texture.
        let layer = |src: &str| object(&[("content", object(&[("src", string(src))]))]);
        let body = object(&[
            ("source", object(&[("tex", string("body"))])),
            ("object", object(&[("body", layer("tex"))])),
        ]);
        let mut head = object(&[
            (
                "source",
                object(&[("tex", string("head")), ("tex@1", string("taken"))]),
            ),
            (
                "object",
                object(&[("head", layer("tex")), ("nested", layer("motion/body/x"))]),
            ),
        ]);
        rename_colliding_sources(&body, &mut head);
        let sources = head.field("source").unwrap();
        assert_eq!(sources.field_str("tex@2"), Some("head"));
        assert_eq!(sources.field_str("tex@1"), Some("taken"));
        assert!(sources.field("tex").is_none());
        let src = |name: &str| {
            head.field("object")
                .unwrap()
                .field(name)
                .unwrap()
                .field("content")
                .unwrap()
                .field_str("src")
                .map(str::to_owned)
        };
        assert_eq!(src("head").as_deref(), Some("tex@2"));
        assert_eq!(src("nested").as_deref(), Some("motion/body/x"));
    }

    fn texture(index: PsbValue) -> PsbValue {
        object(&[(
            "texture",
            object(&[
                ("pixel", index),
                ("width", PsbValue::Int(1)),
                ("height", PsbValue::Int(1)),
            ]),
        )])
    }

    #[test]
    fn multi_source_keeps_cross_file_motions_and_rebases_resource_pools() {
        let body_motion = object(&[("layer", PsbValue::List(vec![string("motion/head/face")]))]);
        let mut body = archive(object(&[
            (
                "object",
                object(&[(
                    "all_parts",
                    object(&[("motion", object(&[("body", body_motion.clone())]))]),
                )]),
            ),
            (
                "source",
                object(&[("body_tex", texture(PsbValue::Resource(0)))]),
            ),
            ("metadata", PsbValue::Null),
        ]));
        let mut head = archive(object(&[
            (
                "object",
                object(&[
                    ("all_parts", object(&[("motion", object(&[]))])),
                    (
                        "head",
                        object(&[("motion", object(&[("face", object(&[]))]))]),
                    ),
                ]),
            ),
            (
                "source",
                object(&[
                    ("head_tex", texture(PsbValue::Resource(0))),
                    ("head_alias", texture(PsbValue::Int(0))),
                ]),
            ),
            (
                "mesh",
                PsbValue::List(vec![PsbValue::Resource(0), PsbValue::ExtraResource(0)]),
            ),
            ("metadata", PsbValue::Null),
        ]));
        let mut bytes = vec![0, 11, 12, 13];
        append_archive(&mut bytes, &mut body, &[0, 21, 22, 23], &mut head).unwrap();
        let mut common = archive(object(&[
            (
                "object",
                object(&[(
                    "all_parts",
                    object(&[(
                        "motion",
                        object(&[(
                            "timeline",
                            object(&[(
                                "layer",
                                PsbValue::List(vec![string("motion/all_parts/body")]),
                            )]),
                        )]),
                    )]),
                )]),
            ),
            ("source", object(&[])),
            (
                "metadata",
                object(&[(
                    "base",
                    object(&[
                        ("chara", string("all_parts")),
                        ("motion", string("timeline")),
                    ]),
                )]),
            ),
        ]));
        common.resources.clear();
        common.extra_resources.clear();
        append_archive(&mut bytes, &mut body, &[], &mut common).unwrap();

        let schema = EmoteModelSchema::from_psb(&body).unwrap();
        assert_eq!(schema.textures["body_tex"].resource_index, 0);
        assert_eq!(schema.textures["head_tex"].resource_index, 1);
        assert_eq!(schema.textures["head_alias"].resource_index, 1);
        assert_eq!(body.resource_bytes(&bytes, 0), Some([11, 12].as_slice()));
        assert_eq!(body.resource_bytes(&bytes, 1), Some([21, 22].as_slice()));
        assert_eq!(body.extra_resources[1].offset, 7);
        assert_eq!(
            body.root.field("mesh"),
            Some(&PsbValue::List(vec![
                PsbValue::Resource(1),
                PsbValue::ExtraResource(1)
            ]))
        );
        let objects = body.root.field("object").unwrap();
        let motions = objects.field("all_parts").unwrap().field("motion").unwrap();
        assert_eq!(motions.field("body"), Some(&body_motion));
        assert!(
            objects
                .field("head")
                .unwrap()
                .field("motion")
                .unwrap()
                .field("face")
                .is_some()
        );
        assert_eq!(
            entry_motion(&body, &schema).unwrap().as_deref(),
            Some("timeline")
        );
    }

    #[test]
    fn duplicate_motion_is_replaced_as_a_complete_definition() {
        let mut root = object(&[(
            "object",
            object(&[(
                "all_parts",
                object(&[(
                    "motion",
                    object(&[
                        (
                            "pose",
                            object(&[
                                ("layer", PsbValue::List(vec![PsbValue::Int(1)])),
                                ("old_field", PsbValue::Int(1)),
                            ]),
                        ),
                        ("other", object(&[])),
                    ]),
                )]),
            )]),
        )]);
        let replacement = object(&[("layer", PsbValue::List(vec![PsbValue::Int(2)]))]);
        merge_root(
            &mut root,
            object(&[(
                "object",
                object(&[(
                    "all_parts",
                    object(&[("motion", object(&[("pose", replacement.clone())]))]),
                )]),
            )]),
        )
        .unwrap();
        let motions = root
            .field("object")
            .unwrap()
            .field("all_parts")
            .unwrap()
            .field("motion")
            .unwrap();
        assert_eq!(motions.field("pose"), Some(&replacement));
        assert!(motions.field("other").is_some());
    }

    #[test]
    fn easing_curves_keep_source_local_indices_and_negative_sentinels() {
        let mut first = archive(object(&[(
            "easing",
            PsbValue::List(vec![string("body_curve")]),
        )]));
        let inline = PsbValue::List(vec![PsbValue::Float(0.5)]);
        let mut next = archive(object(&[
            ("easing", PsbValue::List(vec![string("head_curve")])),
            (
                "frame",
                object(&[
                    ("ccc", PsbValue::Int(0)),
                    ("wcc", PsbValue::Int(-1)),
                    ("occ", inline.clone()),
                ]),
            ),
        ]));
        append_archive(&mut vec![0; 4], &mut first, &[0; 4], &mut next).unwrap();
        merge_root(
            &mut first.root,
            object(&[("easing", PsbValue::List(vec![]))]),
        )
        .unwrap();
        assert_eq!(
            first.root.field("easing"),
            Some(&PsbValue::List(vec![
                string("body_curve"),
                string("head_curve")
            ]))
        );
        let frame = first.root.field("frame").unwrap();
        assert_eq!(frame.field_i64("ccc"), Some(1));
        assert_eq!(frame.field_i64("wcc"), Some(-1));
        assert_eq!(frame.field("occ"), Some(&inline));
    }

    #[test]
    fn empty_source_list_is_rejected() {
        assert!(
            Player::from_sources(&[], None)
                .unwrap_err()
                .to_string()
                .contains("at least one")
        );
    }
}

#[cfg(test)]
mod field_audit {
    use super::*;
    use std::collections::BTreeSet;

    /// Every (path, key) the game's PSBs contain, for checking which ones
    /// Eluna reads. Object keys that name things (motions, layers, sources,
    /// icons) are folded to `*`.
    #[test]
    #[ignore = "requires EMOTE_SNAPSHOT_PROJECT"]
    fn emote_field_audit() {
        let project = std::path::PathBuf::from(std::env::var("EMOTE_SNAPSHOT_PROJECT").unwrap());
        let key = siglus_assets::key_toml::load_emote_key_from_project_dir(&project).unwrap();
        let mut options = EmoteLoadOptions::default();
        if let Some(key) = key {
            options = options.with_emote_key(key);
        }
        let mut seen = BTreeSet::new();
        let mut values = BTreeMap::<String, BTreeSet<String>>::new();
        fn named(parent: &str) -> bool {
            [
                "object",
                "motion",
                "source",
                "icon",
                "texture",
                "variable",
                "parameter",
            ]
            .iter()
            .any(|name| parent.ends_with(name))
                || parent.ends_with("timeline")
        }
        fn walk(
            value: &PsbValue,
            path: &str,
            seen: &mut BTreeSet<String>,
            values: &mut BTreeMap<String, BTreeSet<String>>,
        ) {
            match value {
                PsbValue::List(items) => items
                    .iter()
                    .for_each(|v| walk(v, &format!("{path}[]"), seen, values)),
                PsbValue::Object(fields) => {
                    let fold = named(path);
                    for (name, value) in fields {
                        let key = if fold { "*" } else { name.as_str() };
                        let child = format!("{path}.{key}");
                        seen.insert(child.clone());
                        walk(value, &child, seen, values);
                    }
                }
                PsbValue::Int(n) => {
                    let set = values.entry(path.to_owned()).or_default();
                    if set.len() < 24 {
                        set.insert(n.to_string());
                    }
                }
                PsbValue::String(s) if s.len() < 24 => {
                    let set = values.entry(path.to_owned()).or_default();
                    if set.len() < 24 {
                        set.insert(format!("{s:?}"));
                    }
                }
                _ => {}
            }
        }
        for entry in std::fs::read_dir(project.join("dat")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "psb") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let (_, psb) = PsbFile::parse_normalized(&bytes, &options.normalize).unwrap();
            walk(&psb.root, "", &mut seen, &mut values);
        }
        for path in &seen {
            let vals = values
                .get(path)
                .map(|v| v.iter().cloned().collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            println!("FIELD {path}\t{vals}");
        }
    }
}
