pub(crate) mod output;

use crate::{
    config::{Deploy, Variant},
    home::Home,
    limits::{CLI_TIMEOUT, SHUTDOWN_GRACE, STATUS_INTERVAL},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, process::Stdio};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    time::Instant,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Binary {
    pub(crate) path: PathBuf,
    pub(crate) hash: String,
    pub(crate) version: String,
}

impl Binary {
    pub(crate) async fn resolve(variant: &Variant) -> Result<Self> {
        let path = match &variant.enco {
            Some(path) => path.clone(),
            None => {
                let path = std::env::var_os("PATH").context("PATH is not set")?;
                std::env::split_paths(&path)
                    .map(|dir| dir.join("enco"))
                    .find(|path| path.is_file())
                    .context("enco is not on PATH; provide a variant file")?
            }
        };
        let path = tokio::fs::canonicalize(path)
            .await
            .context("resolve enco binary")?;
        let bytes = tokio::fs::read(&path).await?;
        let output = Command::new(&path)
            .arg("--version")
            .env_clear()
            .stdin(Stdio::null())
            .output()
            .await?;
        ensure!(
            output.status.success(),
            "{} --version failed",
            path.display()
        );
        let version = String::from_utf8(output.stdout)?.trim().to_owned();
        Ok(Self {
            path,
            hash: blake3::hash(&bytes).to_hex().to_string(),
            version,
        })
    }
}

pub(crate) struct Enco {
    binary: PathBuf,
    home: PathBuf,
    environment: BTreeMap<String, OsString>,
    child: Child,
    /// The daemon leads its own process group, which also holds the commands its tools start.
    group: u32,
}

impl Enco {
    pub(crate) fn start(home: &Home, log: std::fs::File) -> Result<Self> {
        let child = Command::new(&home.enco)
            .arg("serve")
            .current_dir(&home.path)
            .env_clear()
            .envs(&home.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .process_group(0)
            .spawn()
            .context("start enco serve")?;
        let group = child.id().context("enco serve has no process ID")?;
        Ok(Self {
            binary: home.enco.clone(),
            home: home.path.clone(),
            environment: home.environment.clone(),
            child,
            group,
        })
    }

    /// Startup includes the first registration, which builds plugin sources, so it shares the
    /// trial's deadline.
    pub(crate) async fn ready(&mut self, deadline: Instant) -> Result<()> {
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!("enco serve exited during startup: {status}; see daemon.log");
            }
            match self.command_until(&["status", "--json"], deadline).await {
                Ok(_) => return Ok(()),
                Err(error) if Instant::now() >= deadline => {
                    return Err(error).context("enco serve did not become ready");
                }
                Err(_) => tokio::time::sleep(STATUS_INTERVAL).await,
            }
        }
    }

    /// Every wait on the subject is bounded, so an unresponsive daemon fails its own trial.
    pub(crate) async fn command(&self, args: &[&str]) -> Result<Value> {
        self.command_until(args, Instant::now() + CLI_TIMEOUT).await
    }

    async fn command_until(&self, args: &[&str], deadline: Instant) -> Result<Value> {
        let mut child = Command::new(&self.binary)
            .args(args)
            .current_dir(&self.home)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("enco {}", args.join(" ")))?;
        let mut stdout = child.stdout.take().context("CLI stdout was not piped")?;
        let mut stderr = child.stderr.take().context("CLI stderr was not piped")?;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let output = async {
            let (status, read_out, read_err) = tokio::join!(
                child.wait(),
                stdout.read_to_end(&mut out),
                stderr.read_to_end(&mut err)
            );
            read_out?;
            read_err?;
            status
        };
        let status = tokio::time::timeout_at(deadline, output)
            .await
            .with_context(|| format!("enco {} timed out", args.join(" ")))??;
        ensure!(
            status.success(),
            "enco {}: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&err).trim()
        );
        serde_json::from_slice(&out).with_context(|| {
            format!(
                "enco {} returned invalid JSON",
                args.first().copied().unwrap_or("")
            )
        })
    }

    pub(crate) async fn deploy(&self, deploy: &Deploy) -> Result<()> {
        let artifact = deploy
            .artifact
            .to_str()
            .context("artifact path is not UTF-8")?;
        self.command(&["plugin", "deploy", &deploy.plugin, artifact, "--json"])
            .await
            .with_context(|| format!("deploy {}", deploy.plugin))?;
        Ok(())
    }

    pub(crate) async fn send(&self, session: &str, text: &str, deadline: Instant) -> Result<()> {
        ensure!(
            Instant::now() < deadline,
            "trial timed out before the next input"
        );
        let args = ["send", "--session", session, "--wait", "--", text];
        self.command_until(&args, deadline).await.map(|_| ())
    }

    pub(crate) async fn idle(&self, deadline: Instant) -> Result<()> {
        let mut timed_out = false;
        loop {
            let status: output::Status =
                serde_json::from_value(self.command(&["status", "--json"]).await?)?;
            let running: Vec<_> = status.node.sessions.iter().filter(|s| s.running).collect();
            if running.is_empty() {
                ensure!(
                    !timed_out,
                    "trial timed out while waiting for Sessions to become idle"
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                ensure!(
                    Instant::now() < deadline + SHUTDOWN_GRACE,
                    "cancelled Runs did not settle within {SHUTDOWN_GRACE:?}"
                );
                for session in running {
                    self.command(&["cancel", "--session", &session.session.id])
                        .await?;
                }
                timed_out = true;
            }
            tokio::time::sleep(STATUS_INTERVAL).await;
        }
    }

    /// Asks the daemon and its tool commands to stop; dropping `Enco` kills what remains.
    pub(crate) async fn stop(mut self) -> Result<()> {
        self.signal("TERM");
        if let Ok(status) = tokio::time::timeout(SHUTDOWN_GRACE, self.child.wait()).await {
            status?;
        }
        Ok(())
    }

    fn signal(&self, name: &str) {
        // An empty group has nothing left to stop, so a failed `kill` needs no handling.
        let _ = std::process::Command::new("kill")
            .args([format!("-{name}"), "--".into(), format!("-{}", self.group)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl Drop for Enco {
    fn drop(&mut self) {
        self.signal("KILL");
    }
}
