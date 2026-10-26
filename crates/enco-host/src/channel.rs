//! Connections between a messaging protocol and Sessions.
//!
//! A connection owns its routing state; adapters own only protocol interpretation.
mod connection;
mod health;
mod projection;
mod settings;
mod supervisor;
mod transport;
pub use settings::{ConnectionSettings, TABLE_OWNER};
pub use supervisor::ChannelSupervisor;

use enco_core::*;
use enco_kernel::{
    ChannelAction as Action, ChannelAdapter as Adapter, ChannelCallError, ChannelFault as Fault,
    ChannelIdentity as Identity, Clock, Kernel, KernelError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Why a connection stopped or could not start.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The kernel failed to accept input or report Session facts.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// The stored connection state cannot be read.
    #[error("invalid connection state: {0}")]
    State(#[from] serde_json::Error),
    /// The protocol failed in a way that stops the connection.
    #[error(transparent)]
    Protocol(#[from] ChannelCallError),
    /// A task of the connection stopped unexpectedly.
    #[error("channel task: {0}")]
    Task(String),
    /// The owner did not complete cleanup, so its helpers may still be running.
    #[error("channel owner did not finish shutdown: {0}")]
    Unjoined(tokio::task::JoinError),
    /// The channel's configuration is invalid, such as a malformed bot token.
    #[error("channel configuration: {0}")]
    Config(String),
}

impl ChannelError {
    fn origin(&self) -> Option<MappingOrigin> {
        match self {
            Self::Protocol(ChannelCallError::Protocol(error)) => error.origin,
            _ => None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    protocol: Option<Value>,
    chats: BTreeMap<String, SessionId>,
    context: BTreeMap<String, Vec<EarlierMessage>>,
    everyone: std::collections::BTreeSet<String>,
    outbound: BTreeMap<SessionId, Cursor>,
    sending: Option<Delivery>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Cursor {
    processed: Option<LogPos>,
    target: Option<String>,
}

/// A connection's health, and the deliveries that failed for good.
#[derive(Serialize)]
pub struct ChannelStatus {
    /// The connection's name in the `channel` table.
    pub name: String,
    /// The account's state key, `<protocol>:<account>`, once the mapper identified it.
    pub key: Option<String>,
    /// Whether the connection is working now.
    pub running: bool,
    /// Why it stopped, when it stopped on a failure.
    pub stopped: Option<String>,
    /// Recorded mapper call responsible for the stopped connection, when one exists.
    pub failure_origin: Option<MappingOrigin>,
    /// Why publishing the protocol command menu failed; the connection continues running.
    pub menu_failure: Option<String>,
    /// The latest deliveries that failed for good.
    pub recent_failures: Vec<enco_kernel::DeliveryStatus>,
}

#[derive(Clone)]
enum Health {
    Running,
    Stopped,
    Failed {
        reason: String,
        origin: Option<MappingOrigin>,
    },
}

/// A running connection.
///
/// Shutdown signals the connection actor and all its helpers, then waits for them.
pub struct Channel {
    name: String,
    key: String,
    kernel: Arc<Kernel>,
    stop: CancellationToken,
    health: Arc<Mutex<Health>>,
    menu_failure: Arc<Mutex<Option<String>>>,
    task: Mutex<Option<JoinHandle<Result<(), ChannelError>>>>,
}

pub(crate) fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Channel {
    /// Start the connection; it carries addressed messages to Sessions and their replies back
    /// until shutdown.
    pub fn start(
        kernel: Arc<Kernel>,
        adapter: Arc<dyn Adapter>,
        settings: ConnectionSettings,
        generation: GenerationId,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let name = settings.name.clone();
        let key = adapter.identity().key();
        let stop = CancellationToken::new();
        let health = Arc::new(Mutex::new(Health::Running));
        let menu_failure = Arc::new(Mutex::new(None));
        let task = tokio::spawn({
            let kernel = kernel.clone();
            let stop = stop.clone();
            let health = health.clone();
            let key = key.clone();
            let menu_failure = menu_failure.clone();
            async move {
                let exit = connection::run(
                    kernel,
                    adapter,
                    settings,
                    generation,
                    clock,
                    stop,
                    menu_failure,
                )
                .await;
                let reason = exit
                    .stopped
                    .as_ref()
                    .or(exit.shutdown.as_ref().err())
                    .map(|error| (error.to_string(), error.origin()));
                *lock(&health) = match reason {
                    Some((reason, origin)) => {
                        tracing::error!(%key, error = %reason, "channel stopped");
                        Health::Failed { reason, origin }
                    }
                    None => Health::Stopped,
                };
                exit.shutdown
            }
        });
        Self {
            name,
            key,
            kernel,
            stop,
            health,
            menu_failure,
            task: Mutex::new(Some(task)),
        }
    }

    /// The connection's health and its latest failed deliveries.
    pub async fn status(&self) -> Result<ChannelStatus, KernelError> {
        let (running, stopped, failure_origin) = match &*lock(&self.health) {
            Health::Running => (true, None, None),
            Health::Stopped => (false, None, None),
            Health::Failed { reason, origin } => (false, Some(reason.clone()), *origin),
        };
        let menu_failure = lock(&self.menu_failure).clone();
        Ok(ChannelStatus {
            name: self.name.clone(),
            key: Some(self.key.clone()),
            running,
            stopped,
            failure_origin,
            menu_failure,
            recent_failures: self.kernel.delivery_failures(&self.key).await?,
        })
    }

    /// Stop the connection and wait until it and its helpers have stopped.
    pub async fn shutdown(&self) -> Result<(), ChannelError> {
        self.stop.cancel();
        let task = lock(&self.task).take();
        if let Some(task) = task {
            task.await.map_err(ChannelError::Unjoined)??;
        }
        Ok(())
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
