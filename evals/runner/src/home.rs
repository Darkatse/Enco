use crate::{
    config::{Model, Trial},
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
        Self { path, environment }
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
        self.environment.extend(model.variables.clone());
        self.environment.extend(variant.variables.clone());
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

    pub(crate) async fn archive(&self, destination: &Path) -> Result<()> {
        if !self.path.try_exists()? {
            return Ok(());
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
