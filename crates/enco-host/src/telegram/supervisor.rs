//! Keeping one Telegram connection in step with the `[telegram]` table of the running
//! configuration.
use super::TelegramSettings;
use crate::channel::{Channel, ChannelError, ChannelStatus, lock};
use enco_kernel::{Clock, Kernel, KernelError, View};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum Current {
    Disabled,
    Running(Arc<Channel>),
    Stopped(String),
}

/// The Telegram connection, restarted whenever the `[telegram]` table changes.
///
/// It waits for the old connection to stop before starting the next one.
pub struct TelegramChannel {
    current: Arc<Mutex<Current>>,
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<Result<(), ChannelError>>>>,
}

impl TelegramChannel {
    /// Start the connection the `[telegram]` table configures, if any, and follow its changes.
    pub fn start(kernel: Arc<Kernel>, clock: Arc<dyn Clock>) -> Self {
        let mut views = kernel.registry().subscribe();
        let document = views.borrow_and_update().config.document.clone();
        let current = Arc::new(Mutex::new(start(&kernel, &clock, &document)));
        let stop = CancellationToken::new();
        let task = tokio::spawn(follow(
            kernel,
            clock,
            current.clone(),
            views,
            table(&document),
            stop.clone(),
        ));
        Self {
            current,
            stop,
            task: Mutex::new(Some(task)),
        }
    }

    /// The connection's status; empty when no `[telegram]` table is configured.
    pub async fn status(&self) -> Result<Vec<ChannelStatus>, KernelError> {
        let current = lock(&self.current).clone();
        Ok(match current {
            Current::Disabled => vec![],
            Current::Running(channel) => vec![channel.status().await?],
            Current::Stopped(reason) => vec![ChannelStatus {
                key: "telegram".into(),
                running: false,
                stopped: Some(reason),
                recent_failures: vec![],
            }],
        })
    }

    /// Stop following the configuration and wait until the connection has stopped.
    pub async fn shutdown(&self) -> Result<(), ChannelError> {
        self.stop.cancel();
        let task = lock(&self.task).take();
        if let Some(task) = task {
            task.await.map_err(ChannelError::Unjoined)??;
        }
        Ok(())
    }
}

impl Drop for TelegramChannel {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// The connection is a function of the active `[telegram]` table; startup is the first view.
async fn follow(
    kernel: Arc<Kernel>,
    clock: Arc<dyn Clock>,
    current: Arc<Mutex<Current>>,
    mut views: watch::Receiver<Arc<View>>,
    mut active: Option<Value>,
    stop: CancellationToken,
) -> Result<(), ChannelError> {
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return shutdown(&current).await,
            changed = views.changed() => {
                if changed.is_err() {
                    return shutdown(&current).await;
                }
                if table(&views.borrow_and_update().config.document) == active {
                    continue;
                }
                // A joined owner is quiescent, so even a failed settlement cannot block the
                // latest table; only an unjoined owner might still be running.
                if let Err(error) = shutdown(&current).await {
                    if matches!(error, ChannelError::Unjoined(_)) {
                        return Err(error);
                    }
                    tracing::error!(%error, "previous Telegram connection stopped with an error");
                }
                if stop.is_cancelled() {
                    return Ok(());
                }
                // Shutdown may await an in-flight send; start only the latest table.
                let document = views.borrow_and_update().config.document.clone();
                active = table(&document);
                *lock(&current) = start(&kernel, &clock, &document);
            }
        }
    }
}

fn table(document: &Value) -> Option<Value> {
    document.get("telegram").cloned()
}

fn start(kernel: &Arc<Kernel>, clock: &Arc<dyn Clock>, document: &Value) -> Current {
    let settings = match TelegramSettings::from_document(document) {
        Ok(Some(settings)) => settings,
        Ok(None) => return Current::Disabled,
        Err(error) => return Current::Stopped(format!("config.invalid: {error}")),
    };
    match settings.adapter() {
        Ok(adapter) => Current::Running(Arc::new(Channel::start(
            kernel.clone(),
            Arc::new(adapter),
            settings.owner_user_id.to_string(),
            clock.clone(),
        ))),
        Err(error) => Current::Stopped(error.to_string()),
    }
}

async fn shutdown(current: &Mutex<Current>) -> Result<(), ChannelError> {
    let snapshot = lock(current).clone();
    if let Current::Running(channel) = snapshot
        && let Err(error) = channel.shutdown().await
    {
        *lock(current) = Current::Stopped(error.to_string());
        return Err(error);
    }
    *lock(current) = Current::Disabled;
    Ok(())
}
