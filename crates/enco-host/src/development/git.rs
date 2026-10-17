//! The registered-intent chain uses a private index and never reads or writes the owner's HEAD.
use super::DevelopmentError;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Output, Stdio},
};
use tokio::{io::AsyncWriteExt, process::Command};

const REGISTERED: &str = "refs/enco/registered";

pub(super) struct Git {
    home: PathBuf,
}

pub(super) enum Snapshot {
    Tree { path: String, tree: String },
    File { path: String, bytes: Vec<u8> },
    Removed(String),
}

impl Snapshot {
    fn path(&self) -> &str {
        match self {
            Self::Tree { path, .. } | Self::File { path, .. } | Self::Removed(path) => path,
        }
    }
}

impl Git {
    pub fn new(home: &Path) -> Self {
        Self { home: home.into() }
    }

    pub async fn check(&self) -> Result<(), DevelopmentError> {
        let root = self.run(&["rev-parse", "--show-toplevel"]).await?;
        if Path::new(&root) != tokio::fs::canonicalize(&self.home).await? {
            return Err(self.error("ENCO_HOME must be the repository root; run enco init"));
        }
        Ok(())
    }

    pub async fn initialize(&self) -> Result<(), DevelopmentError> {
        if !tokio::fs::try_exists(self.home.join(".git")).await? {
            self.run(&["init", "--quiet"]).await?;
        }
        self.check().await
    }

    /// A missing anchor is expected before first boot; other Git failures remain errors.
    pub async fn reference(&self) -> Result<Option<String>, DevelopmentError> {
        let args = ["rev-parse", "--verify", "--quiet", REGISTERED];
        let output = self.command(&args).output().await?;
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        self.check_output(&args, output).map(Some)
    }

    pub async fn anchor(&self, revision: &str) -> Result<(), DevelopmentError> {
        self.run(&["update-ref", REGISTERED, revision]).await?;
        Ok(())
    }

    /// Capture only this directory, including untracked files and honoring ignore rules.
    pub async fn tree(&self, path: &str) -> Result<String, DevelopmentError> {
        let scratch = tempfile::tempdir()?;
        let index = scratch.path().join("index");
        self.indexed(&index, &["add", "--all", "--", path]).await?;
        self.indexed(&index, &["write-tree", &format!("--prefix={path}/")])
            .await
    }

    pub async fn directories(
        &self,
        revision: &str,
    ) -> Result<BTreeMap<String, String>, DevelopmentError> {
        let output = self
            .run(&["ls-tree", "-z", revision, "--", "plugins/"])
            .await?;
        let mut directories = BTreeMap::new();
        for entry in output.split_terminator('\0') {
            let (metadata, path) = entry
                .split_once('\t')
                .ok_or_else(|| self.error("invalid ls-tree entry"))?;
            let Some(name) = path.strip_prefix("plugins/") else {
                continue;
            };
            if !name.contains('/')
                && let Some(hash) = metadata.strip_prefix("040000 tree ")
            {
                directories.insert(name.into(), hash.into());
            }
        }
        Ok(directories)
    }

    pub async fn changed_files(
        &self,
        before: &str,
        after: &str,
    ) -> Result<Vec<String>, DevelopmentError> {
        let output = self
            .run(&[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                before,
                after,
            ])
            .await?;
        Ok(output.lines().map(str::to_owned).collect())
    }

    /// Compose accepted paths over the previous intent, or an empty tree on first boot.
    pub async fn compose(
        &self,
        base: Option<&str>,
        snapshots: &[Snapshot],
    ) -> Result<String, DevelopmentError> {
        let scratch = tempfile::tempdir()?;
        let index = scratch.path().join("index");
        self.indexed(&index, &["read-tree", base.unwrap_or("--empty")])
            .await?;
        for snapshot in snapshots {
            self.stage(&index, snapshot).await?;
        }
        self.indexed(&index, &["write-tree"]).await
    }

    /// Every accepted registration has its own commit, including unchanged trees.
    pub async fn commit(
        &self,
        parent: Option<&str>,
        tree: &str,
        message: &str,
    ) -> Result<String, DevelopmentError> {
        let mut args = vec!["commit-tree", tree, "-m", message];
        if let Some(parent) = parent {
            args.extend(["-p", parent]);
        }
        self.run(&args).await
    }

    async fn stage(&self, index: &Path, snapshot: &Snapshot) -> Result<(), DevelopmentError> {
        self.indexed(
            index,
            &[
                "rm",
                "--cached",
                "--force",
                "-r",
                "--quiet",
                "--ignore-unmatch",
                "--",
                snapshot.path(),
            ],
        )
        .await?;
        match snapshot {
            Snapshot::Tree { path, tree } => {
                self.indexed(index, &["read-tree", &format!("--prefix={path}/"), tree])
                    .await?;
            }
            Snapshot::File { path, bytes } => {
                let blob = self.write_blob(bytes).await?;
                self.indexed(
                    index,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        "100644",
                        &blob,
                        path,
                    ],
                )
                .await?;
            }
            Snapshot::Removed(_) => {}
        }
        Ok(())
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command.arg("-C").arg(&self.home).args([
            "-c",
            "user.name=Enco",
            "-c",
            "user.email=enco@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ]);
        command.args(args);
        command.stdin(Stdio::null()).kill_on_drop(true);
        command
    }

    async fn run(&self, args: &[&str]) -> Result<String, DevelopmentError> {
        let output = self
            .command(args)
            .output()
            .await
            .map_err(|error| self.error(format!("git {}: {error}", args.join(" "))))?;
        self.check_output(args, output)
    }

    /// Each snapshot owns its index, so status reads never wait for a build's workspace lock.
    async fn indexed(&self, index: &Path, args: &[&str]) -> Result<String, DevelopmentError> {
        let output = self
            .command(args)
            .env("GIT_INDEX_FILE", index)
            .output()
            .await
            .map_err(|error| self.error(format!("git {}: {error}", args.join(" "))))?;
        self.check_output(args, output)
    }

    async fn write_blob(&self, bytes: &[u8]) -> Result<String, DevelopmentError> {
        let args = ["hash-object", "-w", "--stdin"];
        let mut child = self
            .command(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| self.error("git stdin is not piped"))?;
        let written = input.write_all(bytes).await;
        drop(input);
        let output = child.wait_with_output().await?;
        let hash = self.check_output(&args, output)?;
        written?;
        Ok(hash)
    }

    fn check_output(&self, args: &[&str], output: Output) -> Result<String, DevelopmentError> {
        if !output.status.success() {
            return Err(self.error(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8(output.stdout).map_err(|error| self.error(error))?;
        Ok(text.trim().to_owned())
    }

    fn error(&self, reason: impl std::fmt::Display) -> DevelopmentError {
        DevelopmentError::Git(format!("{}: {reason}", self.home.display()))
    }
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
