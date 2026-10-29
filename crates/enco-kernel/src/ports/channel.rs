use crate::{Store, StoreError};
use async_trait::async_trait;
use enco_core::{Failure, FileData, GenerationId, MappingOrigin, Outgoing, Unviewable};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Opens accounts through one loaded, pure protocol mapper.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Bind transport settings and identify the account, retaining this generation for all calls.
    async fn open(
        &self,
        settings: &ChannelSettings,
        generation: GenerationId,
        store: Arc<dyn Store>,
    ) -> Result<Arc<dyn ChannelAdapter>, ChannelCallError>;
}

/// The service and secret names chosen for one connection; values remain in transport.
#[derive(Clone)]
pub struct ChannelSettings {
    /// HTTP origin and optional base path used by this connection.
    pub base_url: String,
    /// Mapper credential names mapped to environment variable names.
    pub secrets: BTreeMap<String, String>,
}

/// Stable account identity declared by a mapper.
#[derive(Clone)]
pub struct ChannelIdentity {
    /// A nonempty protocol name without colons, such as `telegram`.
    pub protocol: String,
    /// A nonempty, opaque account ID within that protocol; colons are allowed.
    pub account: String,
    /// Protocol facts needed by subsequent mapper calls.
    pub facts: Value,
}

impl ChannelIdentity {
    /// The connection-state key, `<protocol>:<account>`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.protocol, self.account)
    }
}

/// One arrival's messages and protocol checkpoint, delivered in receive order.
pub enum ChannelReading {
    /// Accept the messages in order before committing the optional protocol checkpoint.
    Batch {
        /// Messages emitted by this arrival, each accepted at most once by its key.
        messages: Vec<ChannelMessage>,
        /// The next committed protocol state; absent leaves it unchanged.
        protocol: Option<Value>,
        /// Complete after acceptance when request-based reading must wait before reading again.
        accepted: Option<oneshot::Sender<()>>,
        /// Completes after all receipt acknowledgments finish; answers must wait for it.
        acknowledged: oneshot::Receiver<()>,
    },
    /// Clear a protocol checkpoint rejected by the mapper before reading from its initial state.
    Reset {
        /// Why the checkpoint was rejected.
        failure: Failure,
        /// Complete after the cleared checkpoint is committed.
        accepted: oneshot::Sender<()>,
    },
}

/// One protocol message, its acceptance key and recorded origin.
pub struct ChannelMessage {
    /// Protocol key accepted at most once, including for commands.
    pub key: String,
    /// What this message asks the host to consider.
    pub input: ChannelInput,
    /// The recorded mapper call and this message's position in its result.
    pub origin: MappingOrigin,
}

/// An incoming message, before host authorization or attachment fetching.
pub struct ChannelInput {
    /// The conversation where replies go.
    pub conversation: String,
    /// This message's ID within its conversation.
    pub message: String,
    /// The sender's protocol ID, when reported.
    pub sender: Option<String>,
    /// The sender's display name, when reported.
    pub sender_name: Option<String>,
    /// Whether the conversation is one to one.
    pub direct: bool,
    /// Whether this message addresses the bot.
    pub addressed: bool,
    /// The protocol ID of the message it replies to, when present.
    pub reply_to: Option<String>,
    /// The opaque handle to answer after acceptance and receipt acknowledgment.
    pub respond: Option<String>,
    /// The host action requested by this message.
    pub action: ChannelAction,
}

/// The meaning of an incoming message, independent of protocol command syntax.
pub enum ChannelAction<T = ChannelAttachment> {
    /// Text and attachments for the conversation's Session.
    Message(ChannelContent<T>),
    /// List available Sessions.
    Sessions,
    /// Select or create the named Session.
    SelectSession(String),
    /// Stop the conversation's current work.
    Cancel,
    /// Return to the message named by the input's reply target.
    Backtrack,
    /// Open or close a group to members; absent queries its current setting.
    Everyone(Option<bool>),
}

/// An image still held by the protocol.
pub struct ChannelAttachment {
    /// A description, such as a file name and media type.
    pub what: String,
    /// The mapper's opaque locator for fetching it.
    pub locator: String,
}

/// The result of reading an attachment.
#[derive(Debug)]
pub enum AttachmentContent {
    /// Image bytes to validate and store.
    Image(Vec<u8>),
    /// Why the protocol cannot provide a usable image.
    Unavailable(String),
}

/// Text and attachments before or after fetching.
pub struct ChannelContent<T> {
    /// The message's text.
    pub text: String,
    /// Images in protocol order, before or after attachment fetching.
    pub images: Vec<T>,
    /// Other attachments and why they cannot be viewed.
    pub unviewable: Vec<Unviewable>,
}

impl<T> From<String> for ChannelContent<T> {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
            unviewable: Vec::new(),
        }
    }
}

impl<T> From<&str> for ChannelContent<T> {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

/// A protocol failure and the facts that determine retry and settlement.
#[derive(Debug, thiserror::Error)]
#[error("{}: {}", .failure.code, .failure.message)]
pub struct ChannelFault {
    /// The reported failure.
    pub failure: Failure,
    /// Effects may have occurred, forbidding automatic resend.
    pub unknown: bool,
    /// The connection is unusable, rather than only this destination.
    pub fatal: bool,
    /// Protocol-requested delay before another attempt.
    pub retry_after: Option<Duration>,
    /// Recorded response call when this failure arose while mapping a response.
    pub origin: Option<MappingOrigin>,
}

/// A mapper or protocol failure, distinct from failing to record its evidence.
#[derive(Debug, thiserror::Error)]
pub enum ChannelCallError {
    /// The mapper or remote protocol rejected the operation.
    #[error(transparent)]
    Protocol(Box<ChannelFault>),
    /// Persistence failed; the operation must stop rather than treating this as a protocol refusal.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<ChannelFault> for ChannelCallError {
    fn from(fault: ChannelFault) -> Self {
        Self::Protocol(Box::new(fault))
    }
}

/// The confirmed result of sending one part.
pub struct ChannelSent {
    /// Protocol ID, absent when the protocol does not expose one.
    pub message: Option<String>,
    /// The recorded call interpreting the send response.
    pub origin: MappingOrigin,
}

/// A command offered by the host in protocol menus.
pub struct ChannelCommand {
    /// Which host command this is.
    pub kind: ChannelCommandKind,
    /// Menu explanation supplied by the host.
    pub description: String,
    /// Applies in one-to-one conversations.
    pub direct: bool,
    /// Applies in group conversations.
    pub group: bool,
}

/// The commands understood by the connection owner.
pub enum ChannelCommandKind {
    /// List or select Sessions.
    Session,
    /// Stop current work.
    Cancel,
    /// Return to a previous message.
    Backtrack,
    /// Change who can address the Agent in a group.
    Everyone,
}

/// A Run's current progress, containing only public tool names and outcomes.
#[derive(Clone)]
pub struct ChannelActivity {
    /// Whole seconds elapsed since the Run started.
    pub elapsed_seconds: u32,
    /// The latest tools in their start order, oldest first.
    pub tools: Vec<ChannelToolActivity>,
}

/// One tool call visible in a Run's progress view.
#[derive(Clone)]
pub struct ChannelToolActivity {
    /// The tool's model-visible name, without its arguments.
    pub name: String,
    /// The call's current outcome.
    pub state: ChannelToolState,
}

/// A tool's current state in a progress view.
#[derive(Clone, Copy)]
pub enum ChannelToolState {
    /// The call is still running.
    Running,
    /// The call succeeded.
    Ok,
    /// The call failed.
    Failed,
    /// The call may have taken effect but its outcome is unknown.
    Unknown,
}

/// The time the host leaves before each protocol message sent through one connection.
#[derive(Clone, Copy)]
pub struct ChannelPacingGap {
    /// Minimum milliseconds between the previous send finishing and the next send starting.
    pub minimum_ms: u32,
    /// Maximum extra milliseconds, chosen randomly before every send, including the first.
    pub jitter_ms: u32,
}

/// Doubling waits between attempts of one channel operation, from one second up to
/// [`CHANNEL_MAX_BACKOFF`](crate::CHANNEL_MAX_BACKOFF).
pub struct ChannelBackoff(Duration);

impl Default for ChannelBackoff {
    fn default() -> Self {
        Self(Duration::from_secs(1))
    }
}

impl ChannelBackoff {
    /// The wait before the next attempt: the one the protocol asked for, or else the current
    /// backoff. Either way the backoff then doubles.
    pub fn next(&mut self, asked: Option<Duration>) -> Duration {
        let wait = asked.unwrap_or(self.0);
        self.0 = (self.0 * 2).min(crate::CHANNEL_MAX_BACKOFF);
        wait
    }
}

/// One identified account, using the same generation until it is stopped.
#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    /// The stable account reached by this connection.
    fn identity(&self) -> ChannelIdentity;
    /// The generation's declared gap before each send; absent sends each as soon as it is ready.
    fn pacing(&self) -> Option<ChannelPacingGap>;
    /// Receive arrivals without waiting for preparation; drain receipt acknowledgments on stop.
    async fn receive(
        &self,
        protocol: Option<Value>,
        readings: mpsc::UnboundedSender<ChannelReading>,
        stop: CancellationToken,
    ) -> Result<(), ChannelCallError>;
    /// Fetch an attachment; one whose address from content is outside the mapper's origins is
    /// unavailable. Reads have no external effects.
    async fn fetch(&self, locator: &str) -> Result<AttachmentContent, ChannelCallError>;
    /// Lay out all content as ordered protocol messages; at least one, none empty.
    async fn split(
        &self,
        message: Outgoing<FileData>,
    ) -> Result<Vec<Outgoing<FileData>>, ChannelCallError>;
    /// Send once without internal retries, retaining evidence for the returned outcome.
    async fn send(
        &self,
        target: &str,
        message: &Outgoing<FileData>,
    ) -> Result<ChannelSent, ChannelCallError>;
    /// Offer the host's commands in the protocol's menus.
    async fn commands(&self, commands: &[ChannelCommand]) -> Result<(), ChannelCallError>;
    /// Answer an accepted message after its receipt acknowledgment completes.
    async fn respond(&self, handle: &str, text: Option<&str>) -> Result<(), ChannelCallError>;
    /// Create, refresh or remove an activity view; creation returns its recorded response, if any.
    async fn activity(
        &self,
        target: &str,
        shown: Option<&str>,
        activity: Option<&ChannelActivity>,
    ) -> Result<Option<ChannelSent>, ChannelCallError>;
}
