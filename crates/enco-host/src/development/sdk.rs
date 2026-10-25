use super::{DevelopmentError, tree::FileTree};
use enco_core::ContentHash;
use std::path::Path;

/// The plugin SDK sources embedded in the host binary, written to `sdk/` in each home.
#[derive(Clone)]
pub struct Sdk {
    tree: FileTree,
}

/// The SDK's own lock pins fetching and seeds plugins; it does not control plugin resolution.
const LOCK: &str = "Cargo.lock";

impl Sdk {
    /// Wrap the embedded files, rejecting any path that is not a canonical relative path.
    pub fn new(files: &[(&str, &[u8])]) -> Result<Self, DevelopmentError> {
        Ok(Self {
            tree: FileTree::from_files(files.iter().map(|(path, bytes)| (*path, bytes.to_vec())))?,
        })
    }

    /// Content hash of the sources that can change a plugin build; the SDK's own `Cargo.lock` is
    /// left out.
    pub fn hash(&self) -> ContentHash {
        let mut tree = self.tree.clone();
        tree.files.remove(LOCK);
        tree.hash()
    }

    /// One bundled file, such as the manifest or the lock a new plugin starts from.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.tree.files.get(path).map(Vec::as_slice)
    }

    /// The tested dependency versions a new plugin starts from.
    pub fn lock(&self) -> Option<&[u8]> {
        self.file(LOCK)
    }

    /// Write the SDK to `sdk/` in `home`, replacing whatever is there, edits included.
    pub async fn write(&self, home: &Path) -> Result<(), DevelopmentError> {
        self.tree.project(&home.join("sdk")).await
    }
}

#[cfg(test)]
mod tests {
    use super::Sdk;

    #[test]
    fn build_reuse_follows_sdk_sources_but_not_the_sdk_lock() {
        let sdk = Sdk::new(&[("Cargo.lock", b"tested"), ("src/lib.rs", b"source")]).unwrap();
        let relocked = Sdk::new(&[("Cargo.lock", b"updated"), ("src/lib.rs", b"source")]).unwrap();
        let edited = Sdk::new(&[("Cargo.lock", b"tested"), ("src/lib.rs", b"edited")]).unwrap();
        assert_eq!(sdk.hash(), relocked.hash());
        assert_ne!(sdk.hash(), edited.hash());
    }
}
