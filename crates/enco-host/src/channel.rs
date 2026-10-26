//! Connections between a messaging protocol and Sessions.
//!
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
    /// The protocol, such as `telegram`.
    pub channel: String,
    /// The account the adapter uses on that protocol, such as a bot's ID.
    pub account: String,
}

impl Identity {
    /// The connection's key, `<channel>:<account>`, under which its state and deliveries are kept.
    pub fn key(&self) -> String {
        format!("{}:{}", self.channel, self.account)
    }
}

/// One protocol update with its resulting protocol state.
pub struct Update {
    /// The protocol state after this update, such as the offset to poll from next.
    pub protocol: Value,
    /// What the update brings, if anything the connection reads.
    pub input: Option<Input>,
}

/// One incoming message as the protocol reports it.
///
/// The connection acts only on direct messages from the owner; it ignores the rest.
pub struct Input {
    /// The protocol's ID for the conversation, where replies go.
    pub conversation: String,
    /// The protocol's ID for this message, unique within the conversation.
    pub message: String,
    /// The protocol's ID for the sender, when it reports one.
    pub sender: Option<String>,
    /// Adapter-provided label retained for diagnostics, not interpreted by the connection.
    pub conversation_type: String,
    /// Whether the conversation is one to one, rather than a group.
    pub direct: bool,
    /// What the message asks for; absent when it holds nothing the host understands.
    pub action: Option<Action<Attachment>>,
}

/// What an incoming message asks for, whatever the protocol's command syntax.
pub enum Action<T = Attachment> {
    /// A message for the conversation's Session.
    Message(MessageContent<T>),
    /// List the Sessions to choose from.
    Sessions,
    /// Route this conversation to the Session with this name, opening it if needed.
    SelectSession(String),
    /// Stop the work the conversation's Session is doing.
    Cancel,
    /// Take the conversation back to the message this one replies to, named by the protocol's ID;
    /// absent when it replies to none.
    Backtrack(Option<String>),
}

/// An image attached to a message, still held by the protocol.
pub struct Attachment {
    /// How to name it when telling the owner about it, such as `photo` or a file's name and type.
    pub what: String,
    /// The protocol's handle for fetching it.
    pub locator: String,
}

/// What fetching an attachment produced.
#[derive(Debug)]
pub enum AttachmentContent {
    /// The image's bytes.
    Image(Vec<u8>),
    /// It cannot be used; the text says why.
    Unavailable(String),
}

/// A message's text and attachments; `T` is an attachment before or after fetching.
pub struct MessageContent<T> {
    /// The text as written.
    pub text: String,
    /// Images to show the model.
    pub images: Vec<T>,
    /// Attachments that could not be made into images, each with the reason.
    pub unviewable: Vec<Unviewable>,
}

impl<T> From<String> for MessageContent<T> {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
            unviewable: Vec::new(),
        }
    }
}

impl<T> From<&str> for MessageContent<T> {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

/// A protocol failure, with what the connection needs to decide whether to retry.
///
/// The common retry and settlement path reads it.
#[derive(Debug, thiserror::Error)]
#[error("{}: {}", .failure.code, .failure.message)]
pub struct Fault {
    /// What failed.
    pub failure: Failure,
    /// True when effects may have occurred; this always forbids an automatic resend.
    pub unknown: bool,
    /// True when the whole connection is unusable, rather than just this destination.
    pub fatal: bool,
    /// How long the protocol asks to wait before trying again, when it says.
    pub retry_after: Option<Duration>,
}

/// The protocol side of a channel: reading updates, fetching attachments and sending replies.
///
/// This native boundary stands until channels become plugins. Outbound Markdown and optional files
/// are mapped to the channel protocol here.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Declare the connection identity; it must remain stable for this adapter instance.
    fn identity(&self) -> Identity;
    /// Protocol state used only when no connection state has been accepted yet.
    fn initial_state(&self) -> Value;
    /// Read replayable updates using committed state; cancellation has no external effect.
    /// Each returned update carries the protocol state after accepting that update.
    async fn poll(&self, protocol: Value) -> Result<Vec<Update>, Fault>;
    /// Fetch a protocol-owned attachment after authorization; reads have no external effect.
    async fn fetch(&self, locator: &str) -> Result<AttachmentContent, Fault>;
    /// Preserve all text; attach the file to the last part, or a separate final part.
    /// Each part has text or a file; a file-only message produces exactly one part.
    fn split(&self, message: Outgoing<FileData>) -> Vec<Outgoing<FileData>>;
    /// Perform one bounded attempt, returning the protocol's ID for the sent message.
    /// Never retry internally or drop an in-flight side effect.
    async fn send(&self, target: &str, message: &Outgoing<FileData>) -> Result<String, Fault>;
}

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
    Protocol(#[from] Fault),
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

/// A connection's health, and the deliveries that failed for good.
#[derive(Serialize)]
pub struct ChannelStatus {
    /// The connection's key, `<channel>:<account>`.
    pub key: String,
    /// Whether the connection is working now.
    pub running: bool,
    /// Why it stopped, when it stopped on a failure.
    pub stopped: Option<String>,
    /// The latest deliveries that failed for good, with the Sessions they came from.
    pub recent_failures: Vec<enco_kernel::DeliveryStatus>,
}

#[derive(Clone)]
enum Health {
    Running,
    Stopped,
    Failed(String),
}

/// A running connection.
///
/// Shutdown signals the connection actor and all its helpers, then waits for them.
pub struct Channel {
    key: String,
    kernel: Arc<Kernel>,
    stop: CancellationToken,
    health: Arc<Mutex<Health>>,
    task: Mutex<Option<JoinHandle<Result<(), ChannelError>>>>,
}

pub(crate) fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Channel {
    /// Start the connection; it carries the owner's direct messages to Sessions and their replies
    /// back until shutdown.
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

    /// The connection's health and its latest failed deliveries.
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
