//! The navigation manual of a plugin source.
//!
//! A build renders it with rustdoc while the source is held still, then stores it by that source,
//! so the manual projection reads it from the store and never needs the toolchain.
use super::{
    Development, DevelopmentError,
    build::{TARGET, parse_messages},
    error::io,
    process,
    tree::FileTree,
};
use crate::SqliteStore;
use enco_core::{ContentHash, IntentPath, PluginName};
use enco_kernel::StoreError;
use serde_json::Value;
use std::path::Path;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// Where rustdoc builds, apart from `.data/target`.
///
/// rustdoc's JSON is unstable, so `RUSTC_BOOTSTRAP` unlocks it on the pinned toolchain; builds with
/// and without that variable would recompile each other's dependencies in a shared directory.
const TARGET_DIR: &str = ".data/target-doc";

impl Development {
    /// Render the navigation manual of the source now in `plugins/<name>`.
    ///
    /// The caller has checked the toolchain, and checks afterwards that the source did not change
    /// while rustdoc read it.
    pub(super) async fn navigation(
        &self,
        name: &PluginName,
        cancel: &CancellationToken,
    ) -> Result<FileTree, DevelopmentError> {
        let location = IntentPath::Plugin(name.clone()).locate(&self.home);
        let directory = tokio::fs::canonicalize(&location)
            .await
            .map_err(|error| io(&location, &error))?;
        let manifest = directory.join("Cargo.toml");
        let target = self.home.join(TARGET_DIR);
        let mut command = Command::new("cargo");
        command
            .current_dir(&directory)
            .args([
                "doc",
                "--lib",
                "--no-deps",
                "--target",
                TARGET,
                "--message-format=json-render-diagnostics",
                "--target-dir",
            ])
            .arg(&target)
            .env("RUSTC_BOOTSTRAP", "1")
            .env("RUSTDOCFLAGS", enco_manual::RUSTDOCFLAGS);
        let output = process::run(command, self.sdk.toolchain(), cancel).await?;
        let mut messages = Vec::new();
        parse_messages(&output.stdout, &manifest, &mut messages, &mut None);
        if !output.status.success() {
            let log = [output.stdout.as_slice(), &output.stderr].concat();
            return Err(self
                .stage_failed(name, "doc", &log, messages, &output.stderr)
                .await);
        }
        let library = library(&output.stdout, &manifest).ok_or_else(|| {
            DevelopmentError::Host(format!("cargo doc named no library for {name}"))
        })?;
        let json = target
            .join(TARGET)
            .join("doc")
            .join(format!("{}.json", library.replace('-', "_")));
        let bytes = tokio::fs::read(&json)
            .await
            .map_err(|error| io(&json, &error))?;
        let krate = enco_manual::read(&bytes)
            .map_err(|error| DevelopmentError::Host(format!("{}: {error}", json.display())))?;
        let files = enco_manual::navigation(&krate, "");
        FileTree::from_files(
            files
                .iter()
                .map(|(path, text)| (path.as_str(), text.clone().into_bytes())),
        )
    }

    /// Store `manual` as the navigation manual of `source`.
    pub(super) async fn remember_navigation(
        &self,
        source: ContentHash,
        manual: &FileTree,
    ) -> Result<(), DevelopmentError> {
        let manual = manual.store(self.store.as_ref()).await?;
        Ok(self.store.remember_manual(source, manual).await?)
    }
}

/// The stored navigation manual of `source`, if there is one.
///
/// There is none when no build stored one, or when its blobs, which are removable history, were
/// removed; the next build of the source stores it again.
pub(super) async fn stored(
    store: &SqliteStore,
    source: ContentHash,
) -> Result<Option<FileTree>, DevelopmentError> {
    let Some(manual) = store.manual(source).await? else {
        return Ok(None);
    };
    match FileTree::load(store, manual).await {
        Ok(tree) => Ok(Some(tree)),
        Err(DevelopmentError::Store(StoreError::Blob(_))) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The library that Cargo documented for the plugin manifest.
fn library(stdout: &[u8], manifest: &Path) -> Option<String> {
    stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .find(|message| {
            message["reason"] == "compiler-artifact"
                && message["manifest_path"]
                    .as_str()
                    .is_some_and(|path| Path::new(path) == manifest)
        })
        .and_then(|message| message.pointer("/target/name")?.as_str().map(str::to_owned))
}
