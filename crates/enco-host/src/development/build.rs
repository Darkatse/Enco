//! Building one plugin source into a component.
//!
//! For the same source and SDK, an earlier build is reused while its component is still stored, and
//! only rustdoc may run, when the source has no navigation manual yet. A fresh build runs under the
//! SDK's toolchain and fails when that toolchain cannot build, on any clippy warning, or on a
//! component that does not load with an empty configuration.
//!
//! Either way, the source must hold still while the build reads it: a change to any file other than
//! `Cargo.lock` and the README's generated section fails the build, so the component and the
//! navigation manual are recorded for the source they came from.
use super::{
    BuildFailure, Development, DevelopmentError, EntryError, EntryProblem, ToolchainProblem,
    cache::Build,
    error::{entry, io},
    navigation, process, readme,
    tree::FileTree,
};
use crate::limits::BUILD_DIAGNOSTICS;
use enco_core::{ContentHash, IntentPath, PluginName, PluginSettings, Utc};
use enco_kernel::{ArtifactSource, Store};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// The target every plugin component is compiled for.
pub(super) const TARGET: &str = "wasm32-wasip2";

/// What happened to one plugin source during an apply.
#[derive(Serialize)]
pub struct BuildResult {
    /// Plugin whose source was built or reused.
    pub name: PluginName,
    /// Whether a stored build of the same source and SDK was used, so Cargo did not run.
    pub reused: bool,
    /// Whether the component differs from the registered one, or none is registered; only then is
    /// the new code registered.
    pub code_changed: bool,
    /// At most `BUILD_DIAGNOSTICS` distinct compiler messages, errors first; empty when the build
    /// was reused.
    pub diagnostics: Vec<String>,
    /// Path to Cargo's complete output; absent when the build was reused.
    pub log: Option<PathBuf>,
}

/// How the bytes were obtained, before they are compared with registered code.
pub(super) struct Compiled {
    pub reused: bool,
    pub diagnostics: Vec<String>,
    pub log: Option<PathBuf>,
}

pub(super) struct Built {
    pub artifact: ArtifactSource,
    pub tree: FileTree,
    pub compiled: Compiled,
}

impl Built {
    pub fn result(
        self,
        name: PluginName,
        code_changed: bool,
    ) -> (ArtifactSource, FileTree, BuildResult) {
        (
            self.artifact,
            self.tree,
            BuildResult {
                name,
                reused: self.compiled.reused,
                code_changed,
                diagnostics: self.compiled.diagnostics,
                log: self.compiled.log,
            },
        )
    }
}

impl Development {
    pub(super) async fn build(
        &self,
        name: &PluginName,
        source: FileTree,
        cached: Option<Build>,
        registered: Option<ContentHash>,
        cancel: &CancellationToken,
    ) -> Result<Built, DevelopmentError> {
        let directory = IntentPath::Plugin(name.clone()).locate(&self.home);
        let sdk = self.sdk.hash();
        if let Some(build) = cached {
            let retained = match self.store.artifact(&build.artifact).await {
                Ok(_) => true,
                Err(enco_kernel::StoreError::Artifact(_)) => false,
                Err(error) => return Err(error.into()),
            };
            if Some(build.artifact) == registered || retained {
                // A build stored before manuals existed has none; rustdoc alone adds it.
                if navigation::stored(&self.store, source.hash())
                    .await?
                    .is_none()
                {
                    self.check_toolchain(name, cancel).await?;
                    let manual = self.navigation(name, cancel).await?;
                    self.unchanged(name, &directory, &source).await?;
                    self.remember_navigation(source.hash(), &manual).await?;
                }
                return Ok(Built {
                    artifact: ArtifactSource::Stored(build.artifact),
                    tree: source,
                    compiled: Compiled {
                        reused: true,
                        diagnostics: Vec::new(),
                        log: None,
                    },
                });
            }
        }

        let failed = |failure| DevelopmentError::Build {
            name: name.clone(),
            failure,
        };
        let toolchain = self.check_toolchain(name, cancel).await?;
        let (bytes, compiled) = self.compile(name, cancel).await?;
        let loaded = self
            .runtime
            .load(&bytes, &PluginSettings::default())
            .await
            .map_err(|error| failed(BuildFailure::EmptySettings(error)))?;
        readme::write(&directory, &loaded).await?;
        let manual = self.navigation(name, cancel).await?;
        let after = self.unchanged(name, &directory, &source).await?;
        self.store
            .remember_build(Build {
                source: after.hash(),
                sdk,
                artifact: ContentHash::of(&bytes),
                toolchain,
                created_at: Utc::now(),
            })
            .await?;
        self.remember_navigation(after.hash(), &manual).await?;
        Ok(Built {
            artifact: ArtifactSource::Bytes(bytes),
            tree: after,
            compiled,
        })
    }

    async fn compile(
        &self,
        name: &PluginName,
        cancel: &CancellationToken,
    ) -> Result<(Vec<u8>, Compiled), DevelopmentError> {
        let location = IntentPath::Plugin(name.clone()).locate(&self.home);
        let directory = tokio::fs::canonicalize(&location)
            .await
            .map_err(|error| io(&location, &error))?;
        let failed = |failure| DevelopmentError::Build {
            name: name.clone(),
            failure,
        };
        let mut log = Vec::new();
        let mut messages = Vec::new();
        let mut artifact = None;
        for stage in ["clippy", "build"] {
            let mut command = Command::new("cargo");
            command
                .current_dir(&directory)
                .args([
                    stage,
                    "--lib",
                    "--release",
                    "--message-format=json-render-diagnostics",
                    "--target-dir",
                ])
                .arg(self.home.join(".data/target"));
            if stage == "clippy" {
                command.args(["--", "-D", "warnings"]);
            }
            let output = process::run(command, self.sdk.toolchain(), cancel).await?;
            log.extend_from_slice(&output.stdout);
            log.extend_from_slice(&output.stderr);
            parse_messages(
                &output.stdout,
                &directory.join("Cargo.toml"),
                &mut messages,
                &mut artifact,
            );
            if !output.status.success() {
                return Err(self
                    .stage_failed(name, stage, &log, messages, &output.stderr)
                    .await);
            }
        }
        let log = self.store.blob_path(&self.store.put_blob(&log).await?);
        let Some(artifact) = artifact else {
            return Err(failed(BuildFailure::NoArtifact(log)));
        };
        let bytes = tokio::fs::read(&artifact)
            .await
            .map_err(|error| io(&artifact, &error))?;
        Ok((
            bytes,
            Compiled {
                reused: false,
                diagnostics: excerpts(messages),
                log: Some(log),
            },
        ))
    }

    /// Read the plugin directory again after the build ran in it.
    ///
    /// Fails with `SourceChanged` when anything but `Cargo.lock` and the README's generated section
    /// differs from `source`, the directory as the build first read it.
    async fn unchanged(
        &self,
        name: &PluginName,
        directory: &Path,
        source: &FileTree,
    ) -> Result<FileTree, DevelopmentError> {
        let after = FileTree::read(directory).await?;
        let readme = directory.join(readme::FILE);
        if after
            .changes(source)
            .iter()
            .any(|path| *path != "Cargo.lock" && *path != readme::FILE)
            || authored_readme(&readme, source)? != authored_readme(&readme, &after)?
        {
            return Err(DevelopmentError::Build {
                name: name.clone(),
                failure: BuildFailure::SourceChanged,
            });
        }
        Ok(after)
    }

    /// Store a failed Cargo stage's log and describe the failure by its diagnostics, or by Cargo's
    /// error output when it reported none.
    pub(super) async fn stage_failed(
        &self,
        name: &PluginName,
        stage: &'static str,
        log: &[u8],
        messages: Vec<(bool, String)>,
        stderr: &[u8],
    ) -> DevelopmentError {
        let log = match self.store.put_blob(log).await {
            Ok(hash) => self.store.blob_path(&hash),
            Err(error) => return error.into(),
        };
        let mut diagnostics = excerpts(messages);
        if diagnostics.is_empty() {
            diagnostics.push(String::from_utf8_lossy(stderr).into_owned());
        }
        DevelopmentError::Build {
            name: name.clone(),
            failure: BuildFailure::Cargo {
                stage,
                diagnostics,
                log,
            },
        }
    }

    /// Check that the SDK's toolchain and the plugin target are installed before Cargo runs, and
    /// return the `rustc -vV` text the build record keeps.
    pub(super) async fn check_toolchain(
        &self,
        name: &PluginName,
        cancel: &CancellationToken,
    ) -> Result<String, DevelopmentError> {
        let channel = self.sdk.toolchain();
        let failed = |problem| DevelopmentError::Build {
            name: name.clone(),
            failure: BuildFailure::Toolchain {
                channel: channel.to_owned(),
                problem,
            },
        };
        let Some(version) = self.rustc(&["-vV"], cancel).await? else {
            return Err(failed(ToolchainProblem::Missing));
        };
        let release = version
            .lines()
            .find_map(|line| line.strip_prefix("release: "))
            .ok_or_else(|| {
                DevelopmentError::Host(format!("rustc -vV names no release: {version}"))
            })?;
        if release != channel {
            return Err(failed(ToolchainProblem::Release {
                found: release.to_owned(),
            }));
        }
        let Some(sysroot) = self.rustc(&["--print", "sysroot"], cancel).await? else {
            return Err(failed(ToolchainProblem::Missing));
        };
        let target = Path::new(sysroot.trim_end())
            .join("lib/rustlib")
            .join(TARGET);
        match tokio::fs::try_exists(&target).await {
            Ok(true) => Ok(version),
            Ok(false) => Err(failed(ToolchainProblem::Target)),
            Err(error) => Err(DevelopmentError::Host(format!(
                "{}: {error}",
                target.display()
            ))),
        }
    }

    /// What `rustc` prints, or `None` when it fails. Cargo honors `RUSTC` the same way.
    async fn rustc(
        &self,
        args: &[&str],
        cancel: &CancellationToken,
    ) -> Result<Option<String>, DevelopmentError> {
        let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
        // Any directory gives the same answer: `RUSTUP_TOOLCHAIN` outranks toolchain files.
        command.current_dir(&self.home).args(args);
        let output = process::run(command, self.sdk.toolchain(), cancel).await?;
        if !output.status.success() {
            return Ok(None);
        }
        String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|error| DevelopmentError::Host(error.to_string()))
    }
}

fn authored_readme(path: &Path, tree: &FileTree) -> Result<String, EntryError> {
    let bytes = tree.files.get(readme::FILE).map_or(&[][..], Vec::as_slice);
    let text = std::str::from_utf8(bytes).map_err(|_| entry(path, EntryProblem::NotText))?;
    readme::authored(path, text)
}

pub(super) fn parse_messages(
    bytes: &[u8],
    manifest: &Path,
    diagnostics: &mut Vec<(bool, String)>,
    artifact: &mut Option<PathBuf>,
) {
    for line in bytes.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        match message["reason"].as_str() {
            Some("compiler-message") => {
                let message = &message["message"];
                let level = message["level"].as_str().unwrap_or("diagnostic");
                if !matches!(level, "error" | "warning") {
                    continue;
                }
                let span = message["spans"]
                    .as_array()
                    .and_then(|spans| spans.iter().find(|span| span["is_primary"] == true));
                let location = span.map_or_else(String::new, |span| {
                    format!(
                        "{}:{}:{}: ",
                        span["file_name"].as_str().unwrap_or("source"),
                        span["line_start"],
                        span["column_start"]
                    )
                });
                let code = message
                    .pointer("/code/code")
                    .and_then(Value::as_str)
                    .map_or_else(String::new, |code| format!("[{code}]"));
                diagnostics.push((
                    level == "error",
                    format!(
                        "{location}{level}{code}: {}",
                        message["message"].as_str().unwrap_or("compiler diagnostic")
                    ),
                ));
            }
            Some("compiler-artifact")
                if message["manifest_path"]
                    .as_str()
                    .is_some_and(|path| Path::new(path) == manifest) =>
            {
                if let Some(paths) = message["filenames"].as_array() {
                    *artifact = paths
                        .iter()
                        .filter_map(Value::as_str)
                        .find(|path| path.ends_with(".wasm"))
                        .map(PathBuf::from)
                        .or(artifact.take());
                }
            }
            _ => {}
        }
    }
}

fn excerpts(mut messages: Vec<(bool, String)>) -> Vec<String> {
    messages.sort_by_key(|(error, _)| !error);
    let mut result = Vec::new();
    for (_, message) in messages {
        if !result.contains(&message) {
            result.push(message);
        }
        if result.len() == BUILD_DIAGNOSTICS {
            break;
        }
    }
    result
}
