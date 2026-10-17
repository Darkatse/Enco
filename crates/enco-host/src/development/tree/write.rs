use super::{Directory, FileTree, Ownership, read::Excluded};
use crate::development::{DevelopmentError, error::io};
use std::{collections::BTreeMap, path::PathBuf};

/// A conflict is determined from the same directory observation as the planned writes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WriteConflict {
    Unregistered {
        path: PathBuf,
    },
    /// An ignored entry occupies a path the target writes.
    Occupied {
        path: PathBuf,
    },
    /// An ignored entry keeps the directory from being removed.
    Ignored {
        path: PathBuf,
    },
    /// The target's ignore rules differ, so this ignored entry could become source.
    IgnoreRulesChanged {
        path: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::development) enum Effect {
    Unchanged,
    Written,
    Removed,
}

enum Operation {
    RemoveFile(PathBuf),
    RemoveDirectory(PathBuf),
    CreateDirectory(PathBuf),
    Write { path: PathBuf, bytes: Vec<u8> },
}

/// An executable file operation list. Constructing it never changes the directory.
pub(in crate::development) struct WritePlan {
    operations: Vec<Operation>,
    effect: Effect,
}

impl Directory {
    /// Plan making this directory hold exactly `target`, keeping excluded entries.
    pub fn plan_write(self, target: FileTree) -> Result<WritePlan, Vec<WriteConflict>> {
        let target_paths: BTreeMap<_, _> = target
            .files
            .keys()
            .map(|path| (self.root.join(path), path))
            .collect();
        let rules_changed = target.changes(&self.value).iter().any(|path| {
            std::path::Path::new(path)
                .file_name()
                .is_some_and(|name| name == ".gitignore")
        });
        self.refuse(|path, excluded| {
            if target_paths
                .keys()
                .any(|target| target.starts_with(path) || path.starts_with(target))
            {
                Some(WriteConflict::Occupied { path: path.clone() })
            } else if rules_changed && matches!(excluded, Excluded::Ignored) {
                Some(WriteConflict::IgnoreRulesChanged { path: path.clone() })
            } else {
                None
            }
        })?;
        let mut operations = Vec::new();
        let mut directories = Vec::new();
        for (path, kind) in self.included() {
            if kind.is_dir() {
                let replaced = target_paths.keys().any(|target| path.starts_with(target));
                let unused = self.ownership == Ownership::Complete
                    && path != &self.root
                    && !target_paths.keys().any(|target| target.starts_with(path));
                if replaced || unused {
                    directories.push(path.clone());
                }
            } else if !kind.is_file() || !target_paths.contains_key(path) {
                operations.push(Operation::RemoveFile(path.clone()));
            }
        }
        operations.extend(deepest_first(directories));
        if self
            .entries
            .get(&self.root)
            .is_none_or(|kind| !kind.is_dir())
        {
            operations.push(Operation::CreateDirectory(self.root.clone()));
        }
        for (path, bytes) in target.files {
            if self.value.files.get(&path) != Some(&bytes) {
                operations.push(Operation::Write {
                    path: self.root.join(path),
                    bytes,
                });
            }
        }
        Ok(WritePlan::new(operations, Effect::Written))
    }

    /// Plan removing this directory; every excluded entry blocks it.
    pub fn plan_removal(self) -> Result<WritePlan, Vec<WriteConflict>> {
        self.refuse(|path, _| Some(WriteConflict::Ignored { path: path.clone() }))?;
        let mut operations = Vec::new();
        let mut directories = Vec::new();
        for (path, kind) in self.included() {
            if kind.is_dir() {
                directories.push(path.clone());
            } else {
                operations.push(Operation::RemoveFile(path.clone()));
            }
        }
        operations.extend(deepest_first(directories));
        Ok(WritePlan::new(operations, Effect::Removed))
    }

    fn refuse(
        &self,
        conflict: impl Fn(&PathBuf, &Excluded) -> Option<WriteConflict>,
    ) -> Result<(), Vec<WriteConflict>> {
        let conflicts: Vec<_> = self
            .excluded
            .iter()
            .filter_map(|(path, excluded)| conflict(path, excluded))
            .collect();
        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(conflicts)
        }
    }

    fn included(&self) -> impl Iterator<Item = (&PathBuf, &std::fs::FileType)> {
        self.entries
            .iter()
            .filter(|(path, _)| !self.excluded.contains_key(*path))
    }
}

fn deepest_first(mut directories: Vec<PathBuf>) -> impl Iterator<Item = Operation> {
    directories.sort_by(|a, b| {
        b.components()
            .count()
            .cmp(&a.components().count())
            .then_with(|| a.cmp(b))
    });
    directories.into_iter().map(Operation::RemoveDirectory)
}

impl WritePlan {
    fn new(operations: Vec<Operation>, effect: Effect) -> Self {
        let effect = if operations.is_empty() {
            Effect::Unchanged
        } else {
            effect
        };
        Self { operations, effect }
    }

    /// A configuration file is owned individually, including an existing file link.
    pub fn file(path: PathBuf, current: Option<&[u8]>, target: Vec<u8>) -> Self {
        let operations = if current == Some(target.as_slice()) {
            Vec::new()
        } else {
            vec![Operation::Write {
                path,
                bytes: target,
            }]
        };
        Self::new(operations, Effect::Written)
    }

    pub fn effect(&self) -> Effect {
        self.effect
    }

    pub async fn execute(self, completed: &mut Vec<PathBuf>) -> Result<(), DevelopmentError> {
        for operation in self.operations {
            let (path, result) = match operation {
                Operation::RemoveFile(path) => {
                    let result = tokio::fs::remove_file(&path).await;
                    (path, result)
                }
                Operation::RemoveDirectory(path) => {
                    let result = tokio::fs::remove_dir(&path).await;
                    (path, result)
                }
                Operation::CreateDirectory(path) => {
                    let result = tokio::fs::create_dir_all(&path).await;
                    (path, result)
                }
                Operation::Write { path, bytes } => {
                    let result = crate::file::write(&path, &bytes).await.map(drop);
                    (path, result)
                }
            };
            result.map_err(|error| io(&path, &error))?;
            completed.push(path);
        }
        Ok(())
    }
}
