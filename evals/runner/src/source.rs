//! Variant source bytes are read once, hashed as the variant's identity, and copied unchanged.
use crate::{config::Source, result::SourceRecord};
use anyhow::{Context, Result, ensure};
use ignore::WalkBuilder;
use std::{collections::BTreeMap, fs::Permissions, path::Path};

struct File {
    bytes: Vec<u8>,
    permissions: Permissions,
}

pub(crate) struct PreparedSource {
    pub(crate) record: SourceRecord,
    files: BTreeMap<String, File>,
}

impl PreparedSource {
    pub(crate) async fn read(source: Source) -> Result<Self> {
        tokio::task::spawn_blocking(move || Self::read_directory(source)).await?
    }

    fn read_directory(source: Source) -> Result<Self> {
        ensure!(
            source.directory.is_dir(),
            "source {} is not a directory",
            source.directory.display()
        );
        let mut files = BTreeMap::new();
        let walker = WalkBuilder::new(&source.directory)
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_global(false)
            .git_exclude(false)
            .require_git(false)
            .filter_entry(|entry| entry.file_name() != ".git")
            .build();
        for entry in walker {
            let entry = entry?;
            if let Some(error) = entry.error() {
                anyhow::bail!("source {}: {error}", entry.path().display());
            }
            let kind = entry.file_type().context("source entry has no file type")?;
            if kind.is_dir() {
                continue;
            }
            let path = entry.path();
            ensure!(
                kind.is_file(),
                "source entry is not a regular file: {}",
                path.display()
            );
            let relative = path
                .strip_prefix(&source.directory)?
                .to_str()
                .context("source path is not UTF-8")?;
            ensure!(
                !relative.contains('\\'),
                "source path contains a backslash: {relative}"
            );
            files.insert(
                relative.to_owned(),
                File {
                    bytes: std::fs::read(path)
                        .with_context(|| format!("read source {}", path.display()))?,
                    permissions: entry.metadata()?.permissions(),
                },
            );
        }
        let manifest: BTreeMap<_, _> = files
            .iter()
            .map(|(path, file)| (path, blake3::hash(&file.bytes).to_hex().to_string()))
            .collect();
        let hash = blake3::hash(&serde_json::to_vec(&manifest)?)
            .to_hex()
            .to_string();
        Ok(Self {
            record: SourceRecord {
                plugin: source.plugin,
                directory: source.directory,
                hash,
            },
            files,
        })
    }

    pub(crate) async fn write(&self, plugins: &Path) -> Result<()> {
        let directory = plugins.join(&self.record.plugin);
        match tokio::fs::remove_dir_all(&directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("replace source {}", directory.display()));
            }
        }
        tokio::fs::create_dir_all(&directory).await?;
        for (relative, file) in &self.files {
            let path = directory.join(relative);
            tokio::fs::create_dir_all(path.parent().context("source file parent")?).await?;
            tokio::fs::write(&path, &file.bytes).await?;
            tokio::fs::set_permissions(&path, file.permissions.clone()).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prepared_source_keeps_exact_bytes_and_ignores_local_builds() {
        let root = std::env::temp_dir().join(format!("enco-source-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(root.join("Cargo.toml"), "{{literal Rust braces}}").unwrap();
        std::fs::write(root.join("target/cache"), "ignored").unwrap();
        let prepared = PreparedSource::read(Source {
            plugin: "fixture".into(),
            directory: root.clone(),
        })
        .await
        .unwrap();
        std::fs::write(root.join("target/cache"), "another local build").unwrap();
        let same = PreparedSource::read(Source {
            plugin: "fixture".into(),
            directory: root.clone(),
        })
        .await
        .unwrap();
        assert_eq!(same.record.hash, prepared.record.hash);
        std::fs::write(root.join("Cargo.toml"), "changed after preparation").unwrap();
        let changed = PreparedSource::read(Source {
            plugin: "fixture".into(),
            directory: root.clone(),
        })
        .await
        .unwrap();
        assert_ne!(changed.record.hash, prepared.record.hash);
        let plugins = root.join("trial/plugins");
        std::fs::create_dir_all(plugins.join("fixture")).unwrap();
        std::fs::write(plugins.join("fixture/old.rs"), "old fixture").unwrap();
        prepared.write(&plugins).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(plugins.join("fixture/Cargo.toml")).unwrap(),
            "{{literal Rust braces}}"
        );
        assert!(!plugins.join("fixture/target").exists());
        assert!(!plugins.join("fixture/old.rs").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
