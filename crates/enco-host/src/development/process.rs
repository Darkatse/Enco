//! A build owns its process group and waits for it to stop before another operation may change
//! the plugin's files.
use super::DevelopmentError;
use std::{
    os::unix::process::CommandExt,
    process::{ExitStatus, Stdio},
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

pub(super) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(super) async fn run(
    mut command: Command,
    cancel: &CancellationToken,
) -> Result<Output, DevelopmentError> {
    if cancel.is_cancelled() {
        return Err(DevelopmentError::Cancelled);
    }
    command.as_std_mut().process_group(0);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(host)?;
    let pid = child
        .id()
        .ok_or_else(|| DevelopmentError::Host("build process has no PID".into()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| DevelopmentError::Host("build stdout is not piped".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| DevelopmentError::Host("build stderr is not piped".into()))?;
    let stdout = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    });
    let stderr = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    });
    let (status, cancelled) = tokio::select! {
        status = child.wait() => (status.map_err(host)?, false),
        _ = cancel.cancelled() => {
            let killed = Command::new("/bin/kill").args(["-KILL", "--", &format!("-{pid}")]).output().await.map_err(host)?;
            if !killed.status.success() && child.try_wait().map_err(host)?.is_none() {
                return Err(DevelopmentError::Host(format!("cannot stop build group {pid}: {}", String::from_utf8_lossy(&killed.stderr))));
            }
            (child.wait().await.map_err(host)?, true)
        }
    };
    let stdout = stdout.await;
    let stderr = stderr.await;
    let stdout = stdout.map_err(host)?.map_err(host)?;
    let stderr = stderr.map_err(host)?.map_err(host)?;
    if cancelled {
        return Err(DevelopmentError::Cancelled);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// The build process is the host's own machinery; its failures are host failures.
fn host(error: impl std::fmt::Display) -> DevelopmentError {
    DevelopmentError::Host(error.to_string())
}
