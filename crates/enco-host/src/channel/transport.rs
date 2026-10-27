//! Ordered preparation, authorized attachment reads and bounded sends. The connection alone commits state.
use super::{Action, Adapter, ChannelCallError, ChannelError, Fault};
use crate::limits::{CHANNEL_ATTEMPTS, CHANNEL_MAX_BACKOFF};
use enco_core::*;
use enco_kernel::{
    AttachmentContent, ChannelAction, ChannelContent, ChannelInput, ImageError, Kernel,
};
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub(super) struct Policy {
    pub everyone: BTreeSet<String>,
}
pub(super) struct Batch {
    pub change: Change,
    pub committed: oneshot::Sender<Policy>,
}
pub(super) enum Change {
    Message(Box<Prepared>),
    Checkpoint {
        protocol: Option<Value>,
        accepted: Option<oneshot::Sender<()>>,
        acknowledged: oneshot::Receiver<()>,
    },
    Reset(Failure),
}
pub(super) struct Prepared {
    pub key: String,
    pub origin: MappingOrigin,
    pub disposition: Disposition,
}
/// What one prepared message comes to; exactly one applies.
pub(super) enum Disposition {
    /// Accepted before, so it brings no effects again.
    Duplicate,
    /// Expects an answer that its sender may not have.
    Refused { handle: String },
    /// Drives a Session or a command.
    Input(Input),
    /// Quoted context for the conversation's next input.
    Earlier {
        conversation: String,
        message: EarlierMessage,
    },
    /// Accepted once and otherwise dropped, such as a member's command.
    Ignored,
}
pub(super) struct Input {
    pub conversation: String,
    pub message: String,
    pub sender: String,
    pub sender_name: Option<String>,
    pub owner: bool,
    pub direct: bool,
    pub reply_to: Option<String>,
    pub respond: Option<String>,
    pub action: Action<Image>,
}

pub(super) struct Reader {
    pub adapter: Arc<dyn Adapter>,
    pub kernel: Arc<Kernel>,
    pub settings: super::ConnectionSettings,
    pub generation: GenerationId,
    pub clock: Arc<dyn enco_kernel::Clock>,
}
impl Reader {
    pub async fn prepare(
        self,
        mut policy: Policy,
        mut readings: mpsc::UnboundedReceiver<enco_kernel::ChannelReading>,
        batches: mpsc::Sender<Batch>,
        stop: CancellationToken,
    ) -> Result<(), ChannelError> {
        loop {
            let reading = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                reading = readings.recv() => reading.ok_or_else(|| ChannelError::Task("channel receiver stopped".into()))?,
            };
            match reading {
                enco_kernel::ChannelReading::Reset { failure, accepted } => {
                    let Some(next) = handover(Change::Reset(failure), &batches, &stop).await?
                    else {
                        return Ok(());
                    };
                    policy = next;
                    let _ = accepted.send(());
                }
                enco_kernel::ChannelReading::Batch {
                    messages,
                    protocol,
                    accepted,
                    acknowledged,
                } => {
                    for message in messages {
                        let prepared = tokio::select! {
                            _ = stop.cancelled() => return Ok(()),
                            result = prepare(self.adapter.as_ref(), &self.kernel, &self.settings, &policy, self.clock.as_ref(), self.generation, message) => result?,
                        };
                        let Some(next) =
                            handover(Change::Message(Box::new(prepared)), &batches, &stop).await?
                        else {
                            return Ok(());
                        };
                        policy = next;
                    }
                    let Some(next) = handover(
                        Change::Checkpoint {
                            protocol,
                            accepted,
                            acknowledged,
                        },
                        &batches,
                        &stop,
                    )
                    .await?
                    else {
                        return Ok(());
                    };
                    policy = next;
                }
            }
        }
    }
}

async fn handover(
    change: Change,
    batches: &mpsc::Sender<Batch>,
    stop: &CancellationToken,
) -> Result<Option<Policy>, ChannelError> {
    let (committed, next) = oneshot::channel();
    tokio::select! {
        biased;
        _ = stop.cancelled() => return Ok(None),
        result = batches.send(Batch { change, committed }) => result.map_err(|_| ChannelError::Task("connection stopped".into()))?,
    }
    tokio::select! {
        biased;
        _ = stop.cancelled() => Ok(None),
        result = next => result.map(Some).map_err(|_| ChannelError::Task("message was not accepted".into())),
    }
}

async fn prepare(
    adapter: &dyn Adapter,
    kernel: &Kernel,
    settings: &super::ConnectionSettings,
    policy: &Policy,
    clock: &dyn enco_kernel::Clock,
    generation: GenerationId,
    message: enco_kernel::ChannelMessage,
) -> Result<Prepared, ChannelError> {
    let enco_kernel::ChannelMessage { key, input, origin } = message;
    let disposition = if kernel
        .connection_update_seen(&adapter.identity().key(), &key)
        .await?
    {
        Disposition::Duplicate
    } else {
        let owner = input.sender.as_deref() == Some(&settings.owner);
        // Members of a group open to everyone may ask, but never command.
        let member = !input.direct
            && policy.everyone.contains(&input.conversation)
            && matches!(input.action, Action::Message(_));
        match (input.sender.clone(), input.respond.clone()) {
            (Some(sender), _) if input.addressed && (owner || member) => Disposition::Input(
                prepare_input(adapter, kernel, input, sender, owner, generation).await?,
            ),
            (_, Some(handle)) => Disposition::Refused { handle },
            _ => match input.action {
                Action::Message(content) if !input.direct => Disposition::Earlier {
                    conversation: input.conversation,
                    message: EarlierMessage {
                        received_at: clock.now(),
                        sender: input.sender,
                        sender_name: input.sender_name,
                        owner,
                        text: content.text,
                        attachments: content
                            .images
                            .into_iter()
                            .map(|image| image.what)
                            .chain(content.unviewable.into_iter().map(|item| item.what))
                            .collect(),
                        origin,
                    },
                },
                _ => Disposition::Ignored,
            },
        }
    };
    Ok(Prepared {
        key,
        origin,
        disposition,
    })
}

async fn prepare_input(
    adapter: &dyn Adapter,
    kernel: &Kernel,
    input: ChannelInput,
    sender: String,
    owner: bool,
    generation: GenerationId,
) -> Result<Input, ChannelError> {
    let action = match input.action {
        ChannelAction::Message(content) => {
            Action::Message(fetch(adapter, kernel, generation, content).await?)
        }
        ChannelAction::Sessions => Action::Sessions,
        ChannelAction::SelectSession(name) => Action::SelectSession(name),
        ChannelAction::Cancel => Action::Cancel,
        ChannelAction::Backtrack => Action::Backtrack,
        ChannelAction::Everyone(open) => Action::Everyone(open),
    };
    Ok(Input {
        conversation: input.conversation,
        message: input.message,
        sender,
        sender_name: input.sender_name,
        owner,
        direct: input.direct,
        reply_to: input.reply_to,
        respond: input.respond,
        action,
    })
}

async fn fetch(
    adapter: &dyn Adapter,
    kernel: &Kernel,
    generation: GenerationId,
    message: ChannelContent<enco_kernel::ChannelAttachment>,
) -> Result<ChannelContent<Image>, ChannelError> {
    let mut prepared = ChannelContent {
        text: message.text,
        images: Vec::new(),
        unviewable: message.unviewable,
    };
    for attachment in message.images {
        let mut delay = Duration::from_secs(1);
        let mut attempt = 1;
        // A transient failure that persists makes the image unavailable rather than holding up
        // every message behind it.
        let fetched = loop {
            match super::health::call(kernel, generation, adapter.fetch(&attachment.locator)).await
            {
                Err(ChannelError::Protocol(ChannelCallError::Protocol(error)))
                    if !error.fatal && error.failure.retryable =>
                {
                    if attempt == CHANNEL_ATTEMPTS {
                        break AttachmentContent::Unavailable(error.failure.message);
                    }
                    attempt += 1;
                    tokio::time::sleep(error.retry_after.unwrap_or(delay)).await;
                    delay = (delay * 2).min(CHANNEL_MAX_BACKOFF);
                }
                result => break result?,
            }
        };
        let bytes = match fetched {
            AttachmentContent::Image(bytes) => bytes,
            AttachmentContent::Unavailable(reason) => {
                prepared.unviewable.push(Unviewable {
                    what: attachment.what,
                    reason: UnviewableReason::Unavailable(reason),
                });
                continue;
            }
        };
        let reason = match kernel.store_image(&bytes).await {
            Ok(image) => {
                prepared.images.push(image);
                continue;
            }
            Err(ImageError::Unsupported) => UnviewableReason::Unsupported,
            Err(ImageError::TooLarge { .. }) => UnviewableReason::TooLarge,
            Err(ImageError::Store(error)) => return Err(ChannelError::Kernel(error.into())),
        };
        prepared.unviewable.push(Unviewable {
            what: attachment.what,
            reason,
        });
    }
    Ok(prepared)
}
pub(super) struct Sent {
    pub outcome: Settlement,
    /// The protocol's IDs for the parts confirmed sent, in order.
    pub messages: Vec<String>,
    pub origins: Vec<MappingOrigin>,
    pub fatal: Option<Fault>,
}

/// Each successful part leaves this loop permanently; unknown effects are never retried.
pub(super) async fn send(
    adapter: Arc<dyn Adapter>,
    kernel: Arc<Kernel>,
    generation: GenerationId,
    target: String,
    message: Outgoing<FileData>,
    stop: CancellationToken,
) -> Result<Sent, ChannelError> {
    let parts = match super::health::call(&kernel, generation, adapter.split(message)).await {
        Ok(parts) => parts,
        Err(ChannelError::Protocol(ChannelCallError::Protocol(error))) => {
            return Ok(Sent {
                outcome: Settlement::Failed {
                    failure: error.failure.clone(),
                },
                messages: Vec::new(),
                origins: Vec::new(),
                fatal: error.fatal.then_some(*error),
            });
        }
        Err(error) => return Err(error),
    };
    let mut origins = Vec::new();
    let mut messages = Vec::with_capacity(parts.len());
    for (part, message) in parts.iter().enumerate() {
        let mut delay = Duration::from_secs(1);
        for attempt in 1..=CHANNEL_ATTEMPTS {
            // A send future has effects: await it even after cancellation.
            match super::health::call(&kernel, generation, adapter.send(&target, message)).await {
                Ok(sent) => {
                    if let Some(id) = sent.message {
                        messages.push(id);
                    }
                    origins.push(sent.origin);
                    break;
                }
                Err(ChannelError::Protocol(ChannelCallError::Protocol(mut error))) => {
                    if let Some(origin) = &error.origin {
                        origins.push(*origin);
                    }
                    if !error.unknown
                        && !error.fatal
                        && error.failure.retryable
                        && attempt < CHANNEL_ATTEMPTS
                        && !wait(error.retry_after.unwrap_or(delay), &stop).await
                    {
                        delay = (delay * 2).min(CHANNEL_MAX_BACKOFF);
                        continue;
                    }
                    if error.unknown {
                        error.failure.retryable = false;
                    }
                    error.failure.message = format!(
                        "part {}/{} ({} earlier parts sent): {}",
                        part + 1,
                        parts.len(),
                        part,
                        error.failure.message
                    );
                    let outcome = if error.unknown {
                        Settlement::Unknown {
                            failure: error.failure.clone(),
                        }
                    } else {
                        Settlement::Failed {
                            failure: error.failure.clone(),
                        }
                    };
                    return Ok(Sent {
                        outcome,
                        messages,
                        origins,
                        fatal: error.fatal.then_some(*error),
                    });
                }
                Err(error) => return Err(error),
            }
        }
        if stop.is_cancelled() && part + 1 < parts.len() {
            return Ok(Sent {
                outcome: Settlement::Failed {
                    failure: Failure {
                        code: code::CANCELLED.into(),
                        message: format!(
                            "channel stopped after sending {} of {} parts",
                            part + 1,
                            parts.len()
                        ),
                        retryable: false,
                    },
                },
                messages,
                origins,
                fatal: None,
            });
        }
    }
    Ok(Sent {
        outcome: Settlement::Ok,
        messages,
        origins,
        fatal: None,
    })
}

async fn wait(delay: Duration, stop: &CancellationToken) -> bool {
    tokio::select! {
        biased;
        _ = stop.cancelled() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}
