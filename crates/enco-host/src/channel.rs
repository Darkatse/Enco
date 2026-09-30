//! A connection owns its routing state; adapters own only protocol interpretation.
mod connection;
mod projection;
mod transport;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{Clock, Kernel, KernelError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Stable identity declared by a protocol adapter.
#[derive(Clone)]
pub struct Identity {
    pub channel: String,
    pub account: String,
}

impl Identity {
    pub fn key(&self) -> String {
        format!("{}:{}", self.channel, self.account)
    }
}

/// One protocol update with its resulting protocol state.
pub struct Update {
    pub protocol: Value,
    pub input: Option<Input>,
}

pub struct Input {
    pub conversation: String,
    pub sender: Option<String>,
    /// Adapter-provided label retained for diagnostics, not interpreted by the connection.
    pub conversation_type: String,
    pub direct: bool,
    /// Unsupported content retains its metadata without producing an action.
    pub action: Option<Action>,
}

/// Meaning interpreted by the host, independent of protocol command syntax.
pub enum Action {
    Message(String),
    Sessions,
    SelectSession(String),
    Cancel,
}

/// Protocol evidence used by the common retry and settlement path.
#[derive(Debug, thiserror::Error)]
#[error("{}: {}", .failure.code, .failure.message)]
pub struct Fault {
    pub failure: Failure,
    /// True when effects may have occurred; this always forbids an automatic resend.
    pub unknown: bool,
    /// True when the whole connection is unusable, rather than just this destination.
    pub fatal: bool,
    pub retry_after: Option<Duration>,
}

/// Native adapter boundary, replaced by the channel plugin boundary in P3.
/// Outbound text is Markdown, authored by its producer and mapped to the channel protocol here.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Declare the connection identity; it must remain stable for this adapter instance.
    fn identity(&self) -> Identity;
    /// Protocol state used only when no connection state has been accepted yet.
    fn initial_state(&self) -> Value;
    /// Read replayable updates using committed state; cancellation has no external effect.
    /// Each returned update carries the protocol state after accepting that update.
    async fn poll(&self, protocol: Value) -> Result<Vec<Update>, Fault>;
    /// Split a nonempty logical message into nonempty protocol-sized parts, preserving all text.
    fn split(&self, text: &str) -> Vec<String>;
    /// Perform one bounded attempt. Never retry internally or drop an in-flight side effect.
    async fn send(&self, target: &str, text: &str) -> Result<(), Fault>;
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error(transparent)]
    Kernel(#[from] KernelError),
    #[error("invalid connection state: {0}")]
    State(#[from] serde_json::Error),
    #[error(transparent)]
    Protocol(#[from] Fault),
    #[error("channel task: {0}")]
    Task(String),
    #[error("channel configuration: {0}")]
    Config(String),
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    protocol: Value,
    chats: BTreeMap<String, SessionId>,
    outbound: BTreeMap<SessionId, Cursor>,
    sending: Option<Delivery>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Cursor {
    processed: Option<LogPos>,
    target: Option<String>,
}

/// Observable connection health and durable final delivery failures.
#[derive(Serialize)]
pub struct ChannelStatus {
    pub key: String,
    pub running: bool,
    pub stopped: Option<String>,
    pub recent_failures: Vec<DeliverySettlement>,
}

#[derive(Clone)]
enum Health {
    Running,
    Stopped,
    Failed(String),
}

/// Lifetime handle; shutdown signals and joins the connection actor and all its helpers.
pub struct Channel {
    key: String,
    kernel: Arc<Kernel>,
    stop: CancellationToken,
    health: Arc<Mutex<Health>>,
    task: Mutex<Option<JoinHandle<Result<(), ChannelError>>>>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Channel {
    pub fn start(
        kernel: Arc<Kernel>,
        adapter: Arc<dyn Adapter>,
        owner_id: String,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let key = adapter.identity().key();
        let stop = CancellationToken::new();
        let health = Arc::new(Mutex::new(Health::Running));
        let task = tokio::spawn({
            let kernel = kernel.clone();
            let stop = stop.clone();
            let health = health.clone();
            let key = key.clone();
            async move {
                let exit = connection::run(kernel, adapter, owner_id, clock, stop).await;
                let reason = exit
                    .stopped
                    .as_ref()
                    .or(exit.shutdown.as_ref().err())
                    .map(ToString::to_string);
                *lock(&health) = match reason {
                    Some(error) => {
                        tracing::error!(%key, %error, "channel stopped");
                        Health::Failed(error)
                    }
                    None => Health::Stopped,
                };
                exit.shutdown
            }
        });
        Self {
            key,
            kernel,
            stop,
            health,
            task: Mutex::new(Some(task)),
        }
    }

    pub async fn status(&self) -> Result<ChannelStatus, KernelError> {
        let (running, stopped) = match &*lock(&self.health) {
            Health::Running => (true, None),
            Health::Stopped => (false, None),
            Health::Failed(reason) => (false, Some(reason.clone())),
        };
        Ok(ChannelStatus {
            key: self.key.clone(),
            running,
            stopped,
            recent_failures: self.kernel.delivery_failures(&self.key).await?,
        })
    }

    pub async fn shutdown(&self) -> Result<(), ChannelError> {
        self.stop.cancel();
        let task = lock(&self.task).take();
        if let Some(task) = task {
            task.await
                .map_err(|error| ChannelError::Task(error.to_string()))??;
        }
        Ok(())
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
