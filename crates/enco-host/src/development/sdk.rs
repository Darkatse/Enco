use super::DevelopmentError;
use enco_core::ContentHash;
use std::path::Path;

/// Plugin-side sources embedded by the composition root, always matching its WIT contract.
#[derive(Clone, Copy)]
pub struct Sdk {
    /// Paths relative to the SDK root and their exact bytes.
    pub files: &'static [(&'static str, &'static [u8])],
}

/// The SDK's own dependency lock applies only when the SDK is the build root.
const LOCK: &str = "Cargo.lock";

impl Sdk {
    /// Address what can change a plugin build. Plugins resolve dependencies from their own lock,
    /// so the SDK's lock is excluded; it pins the SDK's fetch and seeds new plugins instead.
    pub fn hash(&self) -> ContentHash {
        let mut bytes = Vec::new();
        for (path, content) in self.files.iter().filter(|(path, _)| *path != LOCK) {
            bytes.extend_from_slice(&(path.len() as u64).to_le_bytes());
            bytes.extend_from_slice(path.as_bytes());
            bytes.extend_from_slice(&(content.len() as u64).to_le_bytes());
            bytes.extend_from_slice(content);
        }
        ContentHash::of(&bytes)
    }

    /// One bundled file, such as the manifest or the lock a new plugin starts from.
    pub fn file(&self, path: &str) -> Option<&'static [u8]> {
        self.files
            .iter()
            .find(|(file, _)| *file == path)
            .map(|(_, bytes)| *bytes)
    }

    /// The tested dependency versions a new plugin starts from.
    pub fn lock(&self) -> Option<&'static [u8]> {
        self.file(LOCK)
    }

    /// Restore host-owned sources; the SDK directory is a disposable projection.
    pub async fn write(&self, home: &Path) -> Result<(), DevelopmentError> {
        let root = home.join("sdk");
        self.prune(&root).await?;
        for (path, bytes) in self.files {
            let path = root.join(path);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            match tokio::fs::read(&path).await {
                Ok(current) if current == *bytes => continue,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            tokio::fs::write(path, bytes).await?;
        }
        Ok(())
    }

    /// Extra files such as build.rs would otherwise change compilation without changing the SDK hash.
    async fn prune(&self, root: &Path) -> Result<(), std::io::Error> {
        match tokio::fs::symlink_metadata(root).await {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_dir() => {
                return tokio::fs::remove_file(root).await;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
        let mut directories = vec![root.to_path_buf()];
        while let Some(directory) = directories.pop() {
            let mut entries = tokio::fs::read_dir(directory).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
                let kind = entry.file_type().await?;
                if kind.is_file()
                    && self
                        .files
                        .iter()
                        .any(|(file, _)| Path::new(file) == relative)
                {
                    continue;
                }
                if kind.is_dir() {
                    if self
                        .files
                        .iter()
                        .any(|(file, _)| Path::new(file).starts_with(relative))
                    {
                        directories.push(path);
                    } else {
                        tokio::fs::remove_dir_all(path).await?;
                    }
                } else {
                    tokio::fs::remove_file(path).await?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Sdk;

    #[test]
    fn build_reuse_follows_sdk_sources_but_not_the_sdk_lock() {
        let sdk = Sdk {
            files: &[("Cargo.lock", b"tested"), ("src/lib.rs", b"source")],
        };
        let relocked = Sdk {
            files: &[("Cargo.lock", b"updated"), ("src/lib.rs", b"source")],
        };
        let edited = Sdk {
            files: &[("Cargo.lock", b"tested"), ("src/lib.rs", b"edited")],
        };
        assert_eq!(sdk.hash(), relocked.hash());
        assert_ne!(sdk.hash(), edited.hash());
    }

    #[tokio::test]
    async fn restoring_the_bundle_removes_stale_inputs_without_following_symlinks() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("kept"), "owner data").unwrap();
        let old = Sdk {
            files: &[("src/lib.rs", b"old"), ("build.rs", b"stale build")],
        };
        old.write(home.path()).await.unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("sdk/extra")).unwrap();
        let current = Sdk {
            files: &[("src/lib.rs", b"current")],
        };
        current.write(home.path()).await.unwrap();
        assert_eq!(
            std::fs::read(home.path().join("sdk/src/lib.rs")).unwrap(),
            b"current"
        );
        assert!(!home.path().join("sdk/build.rs").exists());
        assert!(!home.path().join("sdk/extra").exists());
        assert_eq!(
            std::fs::read(outside.path().join("kept")).unwrap(),
            b"owner data"
        );
    }
}
