//! A directory as an exact value: file bytes by canonical relative path.
//!
//! A source directory honors only the `.gitignore` files inside it, so its value is the same on
//! every machine. A write to it keeps ignored entries and `.git`, and is refused when one of them
//! is in the way, or when the write changes a `.gitignore` while ignored entries exist. A directory
//! Enco owns entirely, such as `sdk/`, is written exactly and loses everything else.
mod path;
mod read;
mod write;
use super::DevelopmentError;
use super::error::{EntryError, EntryProblem};
use enco_core::ContentHash;
use enco_kernel::Store;
pub use path::{InvalidSourcePath, SourcePath};
pub(super) use read::{Directory, Ownership};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub use write::WriteConflict;
pub(super) use write::{Effect, WritePlan};

/// Exact file bytes keyed by canonical paths relative to a directory.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct FileTree {
    pub(super) files: BTreeMap<SourcePath, Vec<u8>>,
}

impl FileTree {
    pub(super) fn from_files<'a>(
        files: impl IntoIterator<Item = (&'a str, Vec<u8>)>,
    ) -> Result<Self, DevelopmentError> {
        Ok(Self {
            files: files
                .into_iter()
                .map(|(path, bytes)| Ok((path.parse()?, bytes)))
                .collect::<Result<_, DevelopmentError>>()?,
        })
    }

    /// Read source files using only ignore rules owned by this directory.
    pub async fn read(root: &Path) -> Result<Self, DevelopmentError> {
        Ok(Directory::read(root, Ownership::Source).await?.value)
    }

    fn manifest(&self) -> Vec<u8> {
        let files = self
            .files
            .iter()
            .map(|(path, bytes)| {
                (
                    path.to_string(),
                    serde_json::Value::String(ContentHash::of(bytes).to_string()),
                )
            })
            .collect();
        serde_json::Value::Object(files).to_string().into_bytes()
    }

    /// The hash of the canonical compact JSON manifest, with keys in byte order.
    pub fn hash(&self) -> ContentHash {
        ContentHash::of(&self.manifest())
    }

    /// Persist these exact bytes before offering their address to the registry.
    pub(super) async fn store(&self, store: &dyn Store) -> Result<ContentHash, DevelopmentError> {
        for bytes in self.files.values() {
            store.put_blob(bytes).await?;
        }
        Ok(store.put_blob(&self.manifest()).await?)
    }

    /// Resolve a stored tree; canonical paths are checked by manifest deserialization.
    pub(super) async fn load(
        store: &dyn Store,
        source: ContentHash,
    ) -> Result<Self, DevelopmentError> {
        let mut files = BTreeMap::new();
        for (path, hash) in Self::load_manifest(store, source).await? {
            files.insert(path, store.get_blob(&hash).await?);
        }
        Ok(Self { files })
    }

    pub(super) async fn load_manifest(
        store: &dyn Store,
        source: ContentHash,
    ) -> Result<BTreeMap<SourcePath, ContentHash>, DevelopmentError> {
        serde_json::from_slice(&store.get_blob(&source).await?)
            .map_err(|error| DevelopmentError::Corrupt(format!("manifest {source}: {error}")))
    }

    /// File paths whose bytes or presence differ, excluding files already at the target value.
    pub(super) fn changes<'a>(&'a self, current: &'a Self) -> BTreeSet<&'a str> {
        self.files
            .keys()
            .chain(current.files.keys())
            .filter(|path| self.files.get(*path) != current.files.get(*path))
            .map(SourcePath::as_str)
            .collect()
    }

    /// The same planner writes an SDK projection with complete directory ownership.
    pub(super) async fn project(&self, root: &Path) -> Result<(), DevelopmentError> {
        Directory::read(root, Ownership::Complete)
            .await?
            .plan_write(self.clone())
            .map_err(|conflicts| DevelopmentError::WriteConflicts {
                path: root.to_owned(),
                conflicts,
            })?
            .execute(&mut Vec::new())
            .await
    }
}

fn entry(path: &Path, problem: EntryProblem) -> EntryError {
    super::error::entry(path, problem)
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
