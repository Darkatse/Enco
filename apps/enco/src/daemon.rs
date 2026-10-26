use crate::{
    compose_root,
    paths::Paths,
    protocol::{Command, Output, Request, ServerMessage},
};
use anyhow::{Context, Result, bail};
use enco_core::*;
use enco_kernel::{Accepted, ImageError, Kernel, KernelError, Scheduled};
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
    let channel_result = application.channels.shutdown().await;
    let manual_result = application.manual.shutdown().await;
    let (kernel_result, ()) = tokio::join!(kernel.shutdown(), async {
        while connections.join_next().await.is_some() {}
    });
    accept_result?;
    channel_result?;
    manual_result?;
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
                let response = respond(&application, &line, &mut subscription, &stop).await;
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
    stop: &CancellationToken,
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
    match process(application, request.command, subscription, stop).await {
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

#[expect(
    clippy::too_many_lines,
    reason = "one arm per protocol command, so the dispatch reads as the protocol's table"
)]
async fn process(
    application: &compose_root::Application,
    command: Command,
    subscription: &mut Option<Subscription>,
    stop: &CancellationToken,
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
            images,
            event_id,
        } => {
            let mut stored = Vec::new();
            for path in images {
                if !path.is_absolute() {
                    return Err(CommandError {
                        code: "bad_request",
                        message: format!("image path must be absolute: {}", path.display()),
                    });
                }
                let bytes = tokio::fs::read(&path).await.map_err(|error| CommandError {
                    code: "bad_request",
                    message: format!("{}: {error}", path.display()),
                })?;
                stored.push(
                    kernel
                        .store_image(&bytes)
                        .await
                        .map_err(|error| CommandError {
                            code: match error {
                                ImageError::Unsupported | ImageError::TooLarge { .. } => {
                                    "bad_request"
                                }
                                ImageError::Store(_) => "kernel",
                            },
                            message: format!("{}: {error}", path.display()),
                        })?,
                );
            }
            let session = kernel.open_session(&name).await?;
            let accepted = kernel.submit(session.id, event_id, text, stored).await?;
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
                .into_iter()
                .map(Scheduled::into_json)
                .collect::<Vec<_>>()
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
        Command::Status { output } => {
            let status = crate::status::read(application)
                .await
                .map_err(|error| CommandError {
                    code: error.code(),
                    message: error.to_string(),
                })?;
            render(output, &status, || status.document())
        }
        Command::Sessions {} => json!(kernel.sessions().await?),
        Command::Log {
            session: name,
            after,
        } => {
            let session = existing_session(kernel, &name).await?;
            json!(kernel.log(session.id, after).await?)
        }
        Command::Apply { reason, output } => {
            let applied = application
                .development
                .apply(kernel.registry(), Initiator::Owner { reason }, stop)
                .await
                .map_err(development_error)?;
            render(output, &applied, || applied.document())
        }
        Command::History { query, output } => {
            let history = application
                .development
                .history(kernel.registry(), query)
                .await
                .map_err(development_error)?;
            render(output, &history, || history.document())
        }
        Command::Rewind { arguments, output } => {
            let rewound = application
                .development
                .rewind(kernel.registry(), arguments, stop)
                .await
                .map_err(development_error)?;
            render(output, &rewound, || rewound.document())
        }
        Command::Rollback {} => json!(
            kernel
                .registry()
                .rollback()
                .await
                .map_err(KernelError::from)?
        ),
        Command::PluginScaffold { name } => {
            json!({ "files": application.development.scaffold(kernel.registry(), &name).await.map_err(development_error)? })
        }
        Command::PluginRename {
            name,
            to,
            reason,
            output,
        } => {
            let registered = application
                .development
                .rename(
                    kernel.registry(),
                    &name,
                    &to,
                    Initiator::Owner { reason },
                    stop,
                )
                .await
                .map_err(development_error)?;
            render(output, &registered, || {
                enco_host::development::registration_document(&registered)
            })
        }
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

fn development_error(failed: enco_host::development::Failed) -> CommandError {
    CommandError {
        code: failed.code(),
        message: failed.to_string(),
    }
}

fn render(
    output: Output,
    facts: &impl serde::Serialize,
    document: impl FnOnce() -> String,
) -> serde_json::Value {
    match output {
        Output::Json => serde_json::json!(facts),
        Output::Document => document().into(),
    }
}
