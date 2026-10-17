use super::{Development, DevelopmentError, cache::Build, process, readme};
use crate::limits::BUILD_DIAGNOSTICS;
use enco_core::{ContentHash, PluginSettings, Utc};
use enco_kernel::{ArtifactSource, Store};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[derive(Serialize)]
pub struct BuildResult {
    /// Plugin source directory.
    pub name: String,
    /// Whether retained bytes were reused without invoking Cargo.
    pub reused: bool,
    /// Diagnostic excerpts, with errors before warnings.
    pub diagnostics: Vec<String>,
    /// Complete build output, when Cargo was invoked.
    pub log: Option<PathBuf>,
}

pub(super) struct Built {
    pub artifact: ArtifactSource,
    pub tree: String,
    pub result: BuildResult,
}

impl Development {
    pub(super) async fn build(
        &self,
        name: &str,
        source: String,
        registered: Option<ContentHash>,
        cancel: &CancellationToken,
    ) -> Result<Built, DevelopmentError> {
        let relative = format!("plugins/{name}");
        let directory = self.home.join(&relative);
        let sdk = self.sdk.hash();
        if let Some(build) = self.store.build(&source, sdk).await? {
            let retained = match self.store.artifact(&build.artifact).await {
                Ok(_) => true,
                Err(enco_kernel::StoreError::Artifact(_)) => false,
                Err(error) => return Err(error.into()),
            };
            if Some(build.artifact) == registered || retained {
                return Ok(Built {
                    artifact: ArtifactSource::Stored(build.artifact),
                    tree: source,
                    result: BuildResult {
                        name: name.into(),
                        reused: true,
                        diagnostics: Vec::new(),
                        log: None,
                    },
                });
            }
        }
        let readme_before = authored_readme(&directory).await?;
        let (bytes, result) = self.compile(name, cancel).await?;
        let loaded = self
            .runtime
            .load(&bytes, &PluginSettings::default())
            .await
            .map_err(|error| {
                DevelopmentError::Build(format!("{name}: empty configuration is invalid: {error}"))
            })?;
        readme::write(&directory, &loaded).await?;
        let after = self.git.tree(&relative).await?;
        let changed = self.git.changed_files(&source, &after).await?;
        if changed
            .iter()
            .any(|path| path != "Cargo.lock" && path != "README.md")
            || readme_before != authored_readme(&directory).await?
        {
            return Err(DevelopmentError::Build(format!(
                "{name}: source changed during the build; inspect the files and retry"
            )));
        }
        let toolchain = toolchain(&directory, cancel).await?;
        self.store
            .remember_build(Build {
                source: after.clone(),
                sdk,
                artifact: ContentHash::of(&bytes),
                toolchain,
                created_at: Utc::now(),
            })
            .await?;
        Ok(Built {
            artifact: ArtifactSource::Bytes(bytes),
            tree: after,
            result,
        })
    }

    async fn compile(
        &self,
        name: &str,
        cancel: &CancellationToken,
    ) -> Result<(Vec<u8>, BuildResult), DevelopmentError> {
        let directory = tokio::fs::canonicalize(self.home.join("plugins").join(name)).await?;
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
            let output = process::run(command, cancel).await?;
            log.extend_from_slice(&output.stdout);
            log.extend_from_slice(&output.stderr);
            parse_messages(
                &output.stdout,
                &directory.join("Cargo.toml"),
                &mut messages,
                &mut artifact,
            );
            if !output.status.success() {
                let path = self.store.blob_path(&self.store.put_blob(&log).await?);
                let diagnostics = excerpts(messages);
                return Err(DevelopmentError::Build(format!(
                    "{name}: cargo {stage} failed\n{}\nFull log: {}",
                    if diagnostics.is_empty() {
                        String::from_utf8_lossy(&output.stderr).into_owned()
                    } else {
                        diagnostics.join("\n")
                    },
                    path.display()
                )));
            }
        }
        let log = self.store.blob_path(&self.store.put_blob(&log).await?);
        let artifact = artifact.ok_or_else(|| {
            DevelopmentError::Build(format!(
                "{name}: Cargo produced no cdylib Wasm artifact\nFull log: {}",
                log.display()
            ))
        })?;
        let bytes = tokio::fs::read(&artifact).await?;
        let result = BuildResult {
            name: name.into(),
            reused: false,
            diagnostics: excerpts(messages),
            log: Some(log),
        };
        Ok((bytes, result))
    }
}

async fn toolchain(
    directory: &std::path::Path,
    cancel: &CancellationToken,
) -> Result<String, DevelopmentError> {
    let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    command.current_dir(directory).arg("-vV");
    let output = process::run(command, cancel).await?;
    if !output.status.success() {
        return Err(DevelopmentError::Build(format!(
            "rustc -vV: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| DevelopmentError::Build(error.to_string()))
}

async fn authored_readme(directory: &std::path::Path) -> Result<String, DevelopmentError> {
    let text = match tokio::fs::read_to_string(directory.join("README.md")).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    readme::authored(&text)
}

fn parse_messages(
    bytes: &[u8],
    manifest: &std::path::Path,
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
                    .is_some_and(|path| std::path::Path::new(path) == manifest) =>
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
