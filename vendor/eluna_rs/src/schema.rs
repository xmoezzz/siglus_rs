//! Generic PSB schema inspection helpers.
//!
//! These helpers do not assume the Emote model schema. They are for comparing
//! real PSB files against reverse-engineered loader functions.

use crate::PsbValue;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PsbValueKind {
    Null,
    Bool,
    Int,
    Float,
    Double,
    String,
    Resource,
    ExtraResource,
    List,
    Object,
    Compiler,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsbPathEntry {
    pub path: String,
    pub kind: PsbValueKind,
    pub len: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PsbResourceRefs {
    pub resources: BTreeSet<u32>,
    pub extra_resources: BTreeSet<u32>,
}

pub fn psb_value_kind(value: &PsbValue) -> PsbValueKind {
    match value {
        PsbValue::Null => PsbValueKind::Null,
        PsbValue::Bool(_) => PsbValueKind::Bool,
        PsbValue::Int(_) => PsbValueKind::Int,
        PsbValue::Float(_) => PsbValueKind::Float,
        PsbValue::Double(_) => PsbValueKind::Double,
        PsbValue::String(_) => PsbValueKind::String,
        PsbValue::Resource(_) => PsbValueKind::Resource,
        PsbValue::ExtraResource(_) => PsbValueKind::ExtraResource,
        PsbValue::List(_) => PsbValueKind::List,
        PsbValue::Object(_) => PsbValueKind::Object,
        PsbValue::Compiler(_) => PsbValueKind::Compiler,
    }
}

pub fn top_level_keys(root: &PsbValue) -> Vec<&str> {
    match root {
        PsbValue::Object(fields) => fields.iter().map(|(key, _)| key.as_str()).collect(),
        _ => Vec::new(),
    }
}

pub fn collect_schema_paths(root: &PsbValue) -> Vec<PsbPathEntry> {
    let mut out = Vec::new();
    collect_schema_paths_inner(root, "$".to_owned(), &mut out);
    out
}

pub fn collect_resource_refs(root: &PsbValue) -> PsbResourceRefs {
    let mut out = PsbResourceRefs::default();
    collect_resource_refs_inner(root, &mut out);
    out
}

fn collect_schema_paths_inner(value: &PsbValue, path: String, out: &mut Vec<PsbPathEntry>) {
    let len = match value {
        PsbValue::List(values) => Some(values.len()),
        PsbValue::Object(fields) => Some(fields.len()),
        _ => None,
    };

    out.push(PsbPathEntry {
        path: path.clone(),
        kind: psb_value_kind(value),
        len,
    });

    match value {
        PsbValue::List(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_schema_paths_inner(child, format!("{path}[{index}]"), out);
            }
        }
        PsbValue::Object(fields) => {
            for (key, child) in fields {
                collect_schema_paths_inner(child, format!("{path}.{}", escape_path_key(key)), out);
            }
        }
        _ => {}
    }
}

fn collect_resource_refs_inner(value: &PsbValue, out: &mut PsbResourceRefs) {
    match value {
        PsbValue::Resource(index) => {
            out.resources.insert(*index);
        }
        PsbValue::ExtraResource(index) => {
            out.extra_resources.insert(*index);
        }
        PsbValue::List(values) => {
            for child in values {
                collect_resource_refs_inner(child, out);
            }
        }
        PsbValue::Object(fields) => {
            for (_, child) in fields {
                collect_resource_refs_inner(child, out);
            }
        }
        _ => {}
    }
}

fn escape_path_key(key: &str) -> String {
    if key.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        key.to_owned()
    } else {
        format!("[{:?}]", key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_refs() {
        let root = PsbValue::Object(vec![
            ("a".to_owned(), PsbValue::Resource(3)),
            (
                "b".to_owned(),
                PsbValue::List(vec![PsbValue::ExtraResource(7)]),
            ),
        ]);
        let refs = collect_resource_refs(&root);
        assert!(refs.resources.contains(&3));
        assert!(refs.extra_resources.contains(&7));
    }
}
