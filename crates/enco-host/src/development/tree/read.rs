use super::{DevelopmentError, EntryError, EntryProblem, FileTree, entry};
use crate::development::error::io;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    collections::BTreeMap,
    fs::FileType,
    path::{Path, PathBuf},
};

/// Source directories share ownership; SDK projections belong entirely to Enco.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::development) enum Ownership {
    Source,
    Complete,
}

#[derive(Clone, Copy)]
pub(super) enum Excluded {
    Ignored,
    Repository,
}

/// One traversal supplies both the source value and the entries a write must preserve.
pub(in crate::development) struct Directory {
    pub value: FileTree,
    pub(super) root: PathBuf,
    pub(super) ownership: Ownership,
    pub(super) entries: BTreeMap<PathBuf, FileType>,
    pub(super) excluded: BTreeMap<PathBuf, Excluded>,
}

impl Directory {
    pub async fn read(root: &Path, ownership: Ownership) -> Result<Self, DevelopmentError> {
        let root = root.to_owned();
        tokio::task::spawn_blocking(move || Self::read_directory(root, ownership))
            .await
            .map_err(|error| DevelopmentError::Host(error.to_string()))?
            .map_err(DevelopmentError::from)
    }

    pub fn exists(&self) -> bool {
        self.entries.contains_key(&self.root)
    }

    fn read_directory(root: PathBuf, ownership: Ownership) -> Result<Self, EntryError> {
        let mut directory = Self {
            value: FileTree::default(),
            root,
            ownership,
            entries: BTreeMap::new(),
            excluded: BTreeMap::new(),
        };
        let kind = match std::fs::symlink_metadata(&directory.root) {
            Ok(metadata) => metadata.file_type(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(directory),
            Err(error) => return Err(io(&directory.root, &error)),
        };
        directory.entries.insert(directory.root.clone(), kind);
        if kind.is_dir() {
            directory.visit(&directory.root.clone(), &mut Vec::new())?;
        } else if ownership == Ownership::Source {
            return Err(entry(&directory.root, EntryProblem::NotDirectory));
        }
        Ok(directory)
    }

    fn visit(&mut self, directory: &Path, rules: &mut Vec<Gitignore>) -> Result<(), EntryError> {
        let entries = std::fs::read_dir(directory)
            .map_err(|error| io(directory, &error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| io(directory, &error))?;
        let mut local = GitignoreBuilder::new(directory);
        if self.ownership == Ownership::Source {
            for item in &entries {
                let path = item.path();
                if item.file_name() == ".gitignore"
                    && item
                        .file_type()
                        .map_err(|error| io(&path, &error))?
                        .is_file()
                    && let Some(error) = local.add(&path)
                {
                    return Err(entry(&path, EntryProblem::IgnoreRule(error.to_string())));
                }
            }
        }
        rules.push(
            local
                .build()
                .map_err(|error| entry(directory, EntryProblem::IgnoreRule(error.to_string())))?,
        );
        for item in entries {
            let path = item.path();
            let kind = item.file_type().map_err(|error| io(&path, &error))?;
            self.entries.insert(path.clone(), kind);
            let excluded = if item.file_name() == ".git" {
                Some(Excluded::Repository)
            } else if rules
                .iter()
                .rev()
                .map(|rules| rules.matched(&path, kind.is_dir()))
                .find(|matched| !matched.is_none())
                .is_some_and(|matched| matched.is_ignore())
            {
                Some(Excluded::Ignored)
            } else {
                None
            };
            if self.ownership == Ownership::Source
                && let Some(excluded) = excluded
            {
                self.excluded.insert(path, excluded);
                continue;
            }
            if kind.is_dir() {
                self.visit(&path, rules)?;
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(&self.root)
                    .ok()
                    .and_then(std::path::Path::to_str)
                    .ok_or_else(|| entry(&path, EntryProblem::NotUtf8))?;
                let relative = relative
                    .replace(std::path::MAIN_SEPARATOR, "/")
                    .parse()
                    .map_err(|_| entry(&path, EntryProblem::UnsupportedName))?;
                self.value.files.insert(
                    relative,
                    std::fs::read(&path).map_err(|error| io(&path, &error))?,
                );
            } else if self.ownership == Ownership::Source {
                return Err(entry(&path, EntryProblem::NotRegular));
            }
        }
        rules.pop();
        Ok(())
    }
}
