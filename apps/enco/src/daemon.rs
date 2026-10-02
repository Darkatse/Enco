use crate::{
    compose_root,
    paths::Paths,
    protocol::{Command, Request, ServerMessage},
};
use anyhow::{Context, Result, bail};
use enco_core::*;
use enco_kernel::{Accepted, Kernel, KernelError};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream, unix::OwnedWriteHalf},
    sync::broadcast,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub(crate) async fn serve(paths: Paths) -> Result<()> {
    // Hold the same file through startup, shutdown and socket removal. Never unlink it.
    let lock = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(paths.lock())
        .await
        .context("open node lock; run `enco init` first")?
        .into_std()
        .await;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => bail!("enco serve is already running"),
        Err(error) => return Err(error).context("lock node"),
    }
    if let Err(error) = tokio::fs::remove_file(paths.socket()).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).context("remove stale local socket");
    }
    let listener =
        UnixListener::bind(paths.socket()).context("bind local socket; run `enco init` first")?;
    tokio::fs::set_permissions(paths.socket(), std::fs::Permissions::from_mode(0o600)).await?;
    let result = run(listener, &paths).await;
    let _ = tokio::fs::remove_file(paths.socket()).await;
    result
}

async fn run(listener: UnixListener, paths: &Paths) -> Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let application = compose_root::compose(paths).await?;
    let kernel = application.kernel.clone();
    let stop = CancellationToken::new();
    let mut connections = JoinSet::new();
    tracing::info!(socket = %paths.socket().display(), "Enco ready");
    let accept_result: Result<()> = loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = match result {
                    Ok(accepted) => accepted,
                    Err(error) => break Err(error.into()),
                };
                let application = application.clone();
                let stop = stop.clone();
                connections.spawn(async move {
                    if let Err(error) = connection(stream, application, stop).await {
                        tracing::debug!(%error, "local connection ended");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => break Ok(()),
            _ = terminate.recv() => break Ok(()),
            Some(result) = connections.join_next() => {
                if let Err(error) = result {
                    tracing::warn!(%error, "connection task failed");
                }
            }
        }
    };

    // All exits after startup stop accepting work and await the existing owners.
    drop(listener);
    stop.cancel();
    let mut channel_result = Ok(());
    for channel in &application.channels {
        if let Err(error) = channel.shutdown().await {
            channel_result = Err(error);
        }
    }
    let (kernel_result, ()) = tokio::join!(kernel.shutdown(), async {
        while connections.join_next().await.is_some() {}
    });
    accept_result?;
    channel_result?;
    kernel_result?;
    Ok(())
}

struct Subscription {
    session: String,
    entries: broadcast::Receiver<Entry>,
}

async fn connection(
    stream: UnixStream,
    application: Arc<compose_root::Application>,
    stop: CancellationToken,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut subscription: Option<Subscription> = None;
    loop {
        let event = async {
            match &mut subscription {
                Some(s) => s.entries.recv().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            line = lines.next_line() => {
                let Some(line) = line? else { return Ok(()); };
                let response = respond(&application, &line, &mut subscription).await;
                send(&mut writer, &response, &stop).await?;
            }
            entry = event => {
                let message = match entry {
                    Ok(entry) => ServerMessage::Entry {
                        session: subscription.as_ref()
                            .context("entry without subscription")?.session.clone(),
                        entry: Box::new(entry),
                    },
                    Err(broadcast::error::RecvError::Lagged(n)) => ServerMessage::Error {
                        id: 0,
                        code: "lagged".into(),
                        message: format!("missed {n} entries; run `enco log` to resync"),
                    },
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                };
                send(&mut writer, &message, &stop).await?;
            }
        }
    }
}

async fn respond(
    application: &compose_root::Application,
    line: &str,
    subscription: &mut Option<Subscription>,
) -> ServerMessage {
    let request = match serde_json::from_str::<Request>(line) {
        Ok(request) => request,
        Err(error) => {
            return ServerMessage::Error {
                id: 0,
                code: "bad_request".into(),
                message: error.to_string(),
            };
        }
    };
    match process(application, request.command, subscription).await {
        Ok(data) => ServerMessage::Ok {
            id: request.id,
            data,
        },
        Err(error) => ServerMessage::Error {
            id: request.id,
            code: error.code.into(),
            message: error.message,
        },
    }
}

async fn send(
    writer: &mut OwnedWriteHalf,
    message: &ServerMessage,
    stop: &CancellationToken,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    tokio::select! {
        result = writer.write_all(&bytes) => { result?; Ok(()) },
        _ = stop.cancelled() => Ok(()),
    }
}

struct CommandError {
    code: &'static str,
    message: String,
}

impl From<KernelError> for CommandError {
    fn from(error: KernelError) -> Self {
        Self {
            code: if matches!(error, KernelError::UnknownSession(_)) {
                "unknown_session"
            } else {
                "kernel"
            },
            message: error.to_string(),
        }
    }
}

async fn existing_session(kernel: &Kernel, name: &str) -> Result<SessionRecord, CommandError> {
    let sessions = kernel.sessions().await?;
    sessions
        .iter()
        .find(|s| s.name == name)
        .or_else(|| sessions.iter().find(|s| s.id.to_string() == name))
        .cloned()
        .ok_or_else(|| CommandError {
            code: "unknown_session",
            message: format!("unknown session {name}"),
        })
}

async fn process(
    application: &compose_root::Application,
    command: Command,
    subscription: &mut Option<Subscription>,
) -> Result<Value, CommandError> {
    let kernel = &application.kernel;
    let memory_error = |error: enco_host::MemoryError| CommandError {
        code: "memory",
        message: error.to_string(),
    };
    Ok(match command {
        Command::Send {
            session: name,
            text,
            event_id,
        } => {
            let session = kernel.open_session(&name).await?;
            let accepted = kernel.submit(session.id, event_id, text).await?;
            let accepted = match accepted {
                Accepted::New => "new",
                Accepted::Duplicate => "duplicate",
            };
            json!({ "session_id": session.id, "event_id": event_id, "accepted": accepted })
        }
        Command::Subscribe { session: name } => {
            if subscription.is_some() {
                return Err(CommandError {
                    code: "already_subscribed",
                    message: "one Session subscription per connection".into(),
                });
            }
            let session = kernel.open_session(&name).await?;
            *subscription = Some(Subscription {
                session: name,
                entries: kernel.subscribe(session.id)?,
            });
            json!({ "session_id": session.id })
        }
        Command::Cancel { session: name } => {
            let session = existing_session(kernel, &name).await?;
            json!({ "cancelled": kernel.cancel(session.id)? })
        }
        Command::Schedules {} => json!(
            kernel
                .schedules()
                .list(None)
                .await
                .map_err(KernelError::from)?
        ),
        Command::CancelSchedule { schedule_id } => {
            let cancelled = kernel
                .schedules()
                .cancel(schedule_id)
                .await
                .map_err(KernelError::from)?;
            json!({ "cancelled": cancelled })
        }
        Command::Memories {} => json!(application.memories.list().await.map_err(memory_error)?),
        Command::ForgetMemory { memory_id } => {
            let forgotten = application
                .memories
                .forget(memory_id)
                .await
                .map_err(memory_error)?;
            json!({ "forgotten": forgotten })
        }
        Command::Status {} => {
            let mut status = json!(kernel.status().await?);
            let mut channels = Vec::new();
            for channel in &application.channels {
                channels.push(channel.status().await?);
            }
            status["channels"] = json!(channels);
            status
        }
        Command::Sessions {} => json!(kernel.sessions().await?),
        Command::Log {
            session: name,
            after,
        } => {
            let session = existing_session(kernel, &name).await?;
            json!(kernel.log(session.id, after).await?)
        }
        Command::PluginStatus {} => json!(kernel.registry().status().await),
        Command::PluginDeploy { name, path } => {
            let bytes = tokio::fs::read(&path).await.map_err(|error| CommandError {
                code: "bad_request",
                message: format!("{}: {error}", path.display()),
            })?;
            json!(
                kernel
                    .registry()
                    .deploy(&name, bytes)
                    .await
                    .map_err(KernelError::from)?
            )
        }
        Command::PluginRollback { name } => json!(
            kernel
                .registry()
                .rollback(&name)
                .await
                .map_err(KernelError::from)?
        ),
        Command::Inspect {
            session: name,
            attempt_id,
        } => {
            let session = existing_session(kernel, &name).await?;
            json!(kernel.inspect(session.id, attempt_id).await?)
        }
        Command::SetProfile {
            session: name,
            profile,
        } => {
            let session = existing_session(kernel, &name).await?;
            kernel.set_profile(session.id, &profile).await?;
            json!({ "profile": profile })
        }
        Command::SafeMode { enabled } => {
            kernel.set_safe_mode(enabled).await?;
            json!({ "enabled": enabled })
        }
    })
}
