use crate::{
    config::{Model, Trial},
    enco::Binary,
    render::Renderer,
    task::{self, Task},
    trial::Variant,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
};

pub(crate) struct Home {
    pub(crate) path: PathBuf,
    /// The subject's executable, installed where an owner would keep it, so that neither its
    /// path nor the process list points at the evaluator's repository.
    pub(crate) enco: PathBuf,
    pub(crate) environment: BTreeMap<String, OsString>,
}

impl Home {
    pub(crate) fn new(config: &Trial) -> Self {
        let random = ulid::Ulid::generate().to_string()[14..].to_ascii_lowercase();
        let path = config.root.join(random).join(&config.user);
        let enco = path.join(".enco");
        let environment = BTreeMap::from([
            ("HOME".into(), path.as_os_str().to_owned()),
            ("ENCO_HOME".into(), enco.as_os_str().to_owned()),
            ("TMPDIR".into(), path.join("tmp").into_os_string()),
            ("LANG".into(), "C.UTF-8".into()),
            (
                "PATH".into(),
                format!("{}:{}", enco.join("bin").display(), config.path).into(),
            ),
        ]);
        Self {
            enco: path.join(".local/bin/enco"),
            path,
            environment,
        }
    }

    pub(crate) async fn prepare(
        &mut self,
        task: &Task,
        model: &Model,
        renderer: &Renderer,
        proxy: &str,
        mock_hosts: &BTreeSet<String>,
        variant: &Variant,
    ) -> Result<()> {
        let enco = self.path.join(".enco");
        tokio::fs::create_dir_all(enco.join(".data")).await?;
        tokio::fs::create_dir_all(enco.join("workspace")).await?;
        tokio::fs::create_dir_all(self.path.join("tmp")).await?;
        let fixture = task.root.join("home");
        if fixture.try_exists()? {
            copy(&fixture, &enco, renderer).await?;
        }
        let source = model.configuration(
            task.definition.environment.timezone,
            task.definition.budget.max_rounds,
            &variant.plugin,
        )?;
        tokio::fs::write(enco.join("config.toml"), source).await?;
        self.install(&variant.binary).await?;
        self.initialize().await?;
        let cargo = self.toolchain()?;
        self.prefetch(&cargo, &enco).await?;
        self.environment.extend(model.variables.clone());
        self.environment.extend(variant.variables.clone());
        self.environment.extend(
            task.definition
                .environment
                .variables
                .iter()
                .map(|(name, value)| (name.clone(), value.into())),
        );
        for name in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY"] {
            self.environment.insert(name.into(), proxy.into());
        }
        let hosts = model
            .hosts
            .difference(mock_hosts)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(",");
        for name in ["no_proxy", "NO_PROXY"] {
            self.environment.insert(name.into(), hosts.clone().into());
        }
        Ok(())
    }

    /// A hard link costs nothing; a copy covers a home on another file system.
    async fn install(&self, binary: &Binary) -> Result<()> {
        tokio::fs::create_dir_all(self.enco.parent().context("enco directory")?).await?;
        if tokio::fs::hard_link(&binary.path, &self.enco)
            .await
            .is_err()
        {
            tokio::fs::copy(&binary.path, &self.enco)
                .await
                .context("install enco in the trial home")?;
        }
        Ok(())
    }

    async fn initialize(&self) -> Result<()> {
        let initialized = tokio::process::Command::new(&self.enco)
            .arg("init")
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .context("initialize trial intent")?;
        ensure!(
            initialized.status.success(),
            "enco init: {}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        Ok(())
    }

    fn toolchain(&mut self) -> Result<PathBuf> {
        let owner = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
        for (key, default) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
            let path = std::env::var_os(key).map_or_else(|| owner.join(default), PathBuf::from);
            self.environment
                .insert(key.into(), std::path::absolute(path)?.into_os_string());
        }
        let cargo = executable("cargo")?;
        let rustc = executable("rustc")?;
        let path = self
            .environment
            .get("PATH")
            .context("trial PATH is not set")?;
        let mut paths: Vec<_> = std::env::split_paths(path).collect();
        paths.push(cargo.parent().context("cargo directory")?.into());
        self.environment
            .insert("PATH".into(), std::env::join_paths(paths)?);
        self.environment
            .insert("RUSTC".into(), rustc.into_os_string());
        self.environment
            .insert("CARGO_NET_OFFLINE".into(), "true".into());
        Ok(cargo)
    }

    /// Resolve dependencies with the evaluator's network before the subject receives its proxy.
    async fn prefetch(&self, cargo: &Path, enco: &Path) -> Result<()> {
        let mut manifests = vec![enco.join("sdk/Cargo.toml")];
        let mut plugins = tokio::fs::read_dir(enco.join("plugins")).await?;
        while let Some(entry) = plugins.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                manifests.push(entry.path().join("Cargo.toml"));
            }
        }
        manifests.sort();
        for manifest in manifests {
            let output = tokio::process::Command::new(cargo)
                .args(["fetch", "--target", "wasm32-wasip2", "--manifest-path"])
                .arg(&manifest)
                .env("CARGO_HOME", &self.environment["CARGO_HOME"])
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output()
                .await
                .with_context(|| format!("prefetch {}", manifest.display()))?;
            ensure!(
                output.status.success(),
                "prefetch {}: {}",
                manifest.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    pub(crate) async fn archive(&self, destination: &Path) -> Result<()> {
        if !self.path.try_exists()? {
            return Ok(());
        }
        // The variant's hash identifies the executable; the archive need not hold a copy.
        match tokio::fs::remove_file(&self.enco).await {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let output = tokio::process::Command::new("mv")
            .arg(&self.path)
            .arg(destination)
            .stdin(Stdio::null())
            .output()
            .await?;
        ensure!(
            output.status.success(),
            "archive trial home: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
}

impl Drop for Home {
    /// The random parent belongs to this trial alone.
    fn drop(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

async fn copy(source: &Path, destination: &Path, renderer: &Renderer) -> Result<()> {
    for path in task::files(source).await? {
        let target = destination.join(path.strip_prefix(source)?);
        tokio::fs::create_dir_all(target.parent().context("fixture parent")?).await?;
        let bytes = tokio::fs::read(&path).await?;
        let bytes = match std::str::from_utf8(&bytes) {
            Ok(text) => renderer.text(text)?.into_bytes(),
            Err(_) => bytes,
        };
        tokio::fs::write(&target, bytes).await?;
        tokio::fs::set_permissions(&target, tokio::fs::metadata(&path).await?.permissions())
            .await?;
    }
    Ok(())
}

fn executable(name: &str) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").context("PATH is not set")?;
    std::env::split_paths(&path)
        .map(|path| path.join(name))
        .find(|path| path.is_file())
        .with_context(|| format!("{name} is not on the evaluator PATH"))
}
