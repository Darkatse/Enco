use super::{DevelopmentError, tree::FileTree};
use enco_core::ContentHash;
use serde::Deserialize;
use std::path::Path;

/// The plugin SDK sources embedded in the host binary, written to `sdk/` in each home.
#[derive(Clone)]
pub struct Sdk {
    tree: FileTree,
    toolchain: String,
}

/// The SDK's own lock pins fetching and seeds plugins; it does not control plugin resolution.
const LOCK: &str = "Cargo.lock";

/// rustup's toolchain file. rustup looks for it only in a build's directory and its parents, which
/// `sdk/` is not for a plugin, so the host pins each build to its channel instead.
const TOOLCHAIN: &str = "rust-toolchain.toml";

impl Sdk {
    /// Wrap the embedded files, rejecting a path that is not canonical and relative, or a bundle
    /// whose `rust-toolchain.toml` names no channel.
    pub fn new(files: &[(&str, &[u8])]) -> Result<Self, DevelopmentError> {
        let tree = FileTree::from_files(files.iter().map(|(path, bytes)| (*path, bytes.to_vec())))?;
        let toolchain = channel(tree.files.get(TOOLCHAIN).map(Vec::as_slice))?;
        Ok(Self { tree, toolchain })
    }

    /// Content hash of the sources that can change a plugin build, the toolchain included; the
    /// SDK's own `Cargo.lock` is left out.
    pub fn hash(&self) -> ContentHash {
        let mut tree = self.tree.clone();
        tree.files.remove(LOCK);
        tree.hash()
    }

    /// The Rust toolchain every plugin build in a home uses, as rustup names it.
    pub fn toolchain(&self) -> &str {
        &self.toolchain
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

fn channel(file: Option<&[u8]>) -> Result<String, DevelopmentError> {
    #[derive(Deserialize)]
    struct File {
        toolchain: Toolchain,
    }
    #[derive(Deserialize)]
    struct Toolchain {
        channel: String,
    }
    let text = file
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| DevelopmentError::Host(format!("the SDK has no UTF-8 {TOOLCHAIN}")))?;
    toml::from_str::<File>(text)
        .map(|file| file.toolchain.channel)
        .map_err(|error| DevelopmentError::Host(format!("the SDK's {TOOLCHAIN}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::Sdk;

    const TOOLCHAIN: (&str, &[u8]) = (
        "rust-toolchain.toml",
        b"[toolchain]\nchannel = \"1.99.0\"\n",
    );

    #[test]
    fn build_reuse_follows_sdk_sources_but_not_the_sdk_lock() {
        let sdk = Sdk::new(&[
            TOOLCHAIN,
            ("Cargo.lock", b"tested"),
            ("src/lib.rs", b"source"),
        ])
        .unwrap();
        let relocked = Sdk::new(&[
            TOOLCHAIN,
            ("Cargo.lock", b"updated"),
            ("src/lib.rs", b"source"),
        ])
        .unwrap();
        let edited = Sdk::new(&[
            TOOLCHAIN,
            ("Cargo.lock", b"tested"),
            ("src/lib.rs", b"edited"),
        ])
        .unwrap();
        assert_eq!(sdk.hash(), relocked.hash());
        assert_ne!(sdk.hash(), edited.hash());
    }
}
