//! Comparing two sides unit by unit and file by file, with a line diff wherever both are text.
use super::content::{ContentUnavailable, Contents, Unit};
use super::{FileDiff, FileDifference, UnitDiff};
use crate::development::tree::FileTree;
use enco_core::IntentPath;
use std::collections::BTreeMap;

struct FilePair {
    path: IntentPath,
    before: Result<FileTree, ContentUnavailable>,
    after: Result<FileTree, ContentUnavailable>,
}

pub(super) fn compare_contents(before: Contents, after: Contents) -> BTreeMap<Unit, UnitDiff> {
    let mut pairs: BTreeMap<_, _> = before
        .into_iter()
        .map(|(unit, content)| {
            (
                unit,
                FilePair {
                    path: content.path,
                    before: content.value,
                    after: Ok(FileTree::default()),
                },
            )
        })
        .collect();
    for (unit, content) in after {
        match pairs.entry(unit) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().path = content.path;
                entry.get_mut().after = content.value;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(FilePair {
                    path: content.path,
                    before: Ok(FileTree::default()),
                    after: content.value,
                });
            }
        }
    }
    pairs
        .into_iter()
        .filter_map(|(unit, pair)| {
            let plugin = match &unit {
                Unit::Plugin(id) => Some(*id),
                Unit::Configuration | Unit::Unregistered(_) => None,
            };
            match (pair.before, pair.after) {
                (Ok(before), Ok(after)) => {
                    if after.hash() == before.hash() {
                        return None;
                    }
                    Some((
                        unit,
                        UnitDiff {
                            files: diff(&pair.path, &before, &after),
                            path: pair.path,
                            plugin,
                            unavailable: None,
                        },
                    ))
                }
                (Err(reason), _) | (_, Err(reason)) => Some((
                    unit,
                    UnitDiff {
                        path: pair.path,
                        plugin,
                        files: Vec::new(),
                        unavailable: Some(reason),
                    },
                )),
            }
        })
        .collect()
}

fn diff(unit: &IntentPath, before: &FileTree, after: &FileTree) -> Vec<FileDiff> {
    after
        .changes(before)
        .into_iter()
        .map(|path| {
            let old = before.files.get(path).map_or(&[][..], Vec::as_slice);
            let new = after.files.get(path).map_or(&[][..], Vec::as_slice);
            let name = if *unit == IntentPath::Configuration {
                unit.to_string()
            } else {
                format!("{unit}/{path}")
            };
            let change = if old.is_empty() && new.is_empty() {
                if after.files.contains_key(path) {
                    FileDifference::AddedEmpty
                } else {
                    FileDifference::RemovedEmpty
                }
            } else {
                match (std::str::from_utf8(old), std::str::from_utf8(new)) {
                    (Ok(old), Ok(new)) if !old.contains('\0') && !new.contains('\0') => {
                        FileDifference::Text {
                            diff: similar::TextDiff::from_lines(old, new)
                                .unified_diff()
                                .header(
                                    &if before.files.contains_key(path) {
                                        format!("a/{name}")
                                    } else {
                                        "/dev/null".into()
                                    },
                                    &if after.files.contains_key(path) {
                                        format!("b/{name}")
                                    } else {
                                        "/dev/null".into()
                                    },
                                )
                                .to_string(),
                        }
                    }
                    _ => FileDifference::Binary,
                }
            };
            FileDiff { path: name, change }
        })
        .collect()
}
