//! Polling, authorized attachment reads and bounded sends. The connection alone commits state.
use super::{Action, Adapter, ChannelCallError, ChannelError, Fault};
use crate::limits::{CHANNEL_MAX_BACKOFF, CHANNEL_SEND_ATTEMPTS};
use enco_core::*;
use enco_kernel::{
    AttachmentContent, ChannelAction, ChannelContent, ChannelInput, ChannelUpdate, ImageError,
    Kernel,
};
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub(super) struct Policy {
    pub protocol: Option<Value>,
    pub everyone: BTreeSet<String>,
}
pub(super) struct Batch {
    pub change: Change,
    pub committed: oneshot::Sender<Policy>,
}
pub(super) enum Change {
    Update(Box<Prepared>),
    Reset(Failure),
}
pub(super) struct Prepared {
    pub key: String,
    pub duplicate: bool,
    pub protocol: Value,
    pub origin: MappingOrigin,
    pub input: Option<Input>,
    pub earlier: Option<(String, EarlierMessage)>,
}
pub(super) struct Input {
    pub conversation: String,
    pub message: String,
    pub sender: String,
    pub sender_name: Option<String>,
    pub owner: bool,
    pub direct: bool,
    pub reply_to: Option<String>,
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
    pub async fn poll(
        self,
        mut policy: Policy,
        batches: mpsc::Sender<Batch>,
        stop: CancellationToken,
    ) -> Result<(), ChannelError> {
        let Self {
            adapter,
            kernel,
            settings,
            generation,
            clock,
        } = self;
        let mut delay = Duration::from_secs(1);
        loop {
            let result = tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                result = super::health::call(&kernel, generation, adapter.poll(policy.protocol.clone())) => result,
            };
            let result = match result {
                Err(ChannelError::Protocol(ChannelCallError::Protocol(error)))
                    if error.failure.code == code::CHANNEL_INVALID_STATE =>
                {
                    if policy.protocol.is_none() {
                        return Err(ChannelCallError::Protocol(error).into());
                    }
                    let Some(next) =
                        handover(Change::Reset(error.failure), &batches, &stop).await?
                    else {
                        return Ok(());
                    };
                    policy = next;
                    continue;
                }
                result => result,
            };
            let result = async {
            for update in result? {
                let prepared = tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    result = prepare(adapter.as_ref(), &kernel, &settings, &policy, clock.as_ref(), generation, update) => result?,
                };
                let Some(next) = handover(Change::Update(Box::new(prepared)), &batches, &stop).await? else {
                    return Ok(());
                };
                policy = next;
            }
            Ok::<_, ChannelError>(())
        }
        .await;
            match result {
                Ok(()) => delay = Duration::from_secs(1),
                Err(ChannelError::Protocol(ChannelCallError::Protocol(error)))
                    if !error.fatal && error.failure.retryable =>
                {
                    tracing::warn!(%error, "channel polling will retry");
                    if wait(error.retry_after.unwrap_or(delay), &stop).await {
                        return Ok(());
                    }
                    delay = (delay * 2).min(CHANNEL_MAX_BACKOFF);
                }
                Err(error) => return Err(error),
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
        _ = stop.cancelled() => return Ok(None),
        result = batches.send(Batch { change, committed }) => result.map_err(|_| ChannelError::Task("connection stopped".into()))?,
    }
    tokio::select! {
        _ = stop.cancelled() => Ok(None),
        result = next => result.map(Some).map_err(|_| ChannelError::Task("update was not accepted".into())),
    }
}

async fn prepare(
    adapter: &dyn Adapter,
    kernel: &Kernel,
    settings: &super::ConnectionSettings,
    policy: &Policy,
    clock: &dyn enco_kernel::Clock,
    generation: GenerationId,
    update: ChannelUpdate,
) -> Result<Prepared, ChannelError> {
    let mut prepared = Prepared {
        key: update.key,
        duplicate: false,
        protocol: update.protocol,
        origin: update.origin,
        input: None,
        earlier: None,
    };
    if kernel
        .connection_update_seen(&adapter.identity().key(), &prepared.key)
        .await?
    {
        prepared.duplicate = true;
        return Ok(prepared);
    }
    let Some(input) = update.input else {
        return Ok(prepared);
    };
    let owner = input.sender.as_deref() == Some(&settings.owner);
    // Members of a group open to everyone may ask, but never command.
    let member = !input.direct
        && policy.everyone.contains(&input.conversation)
        && matches!(input.action, Action::Message(_));
    if let Some(sender) = input.sender.clone()
        && input.addressed
        && (owner || member)
    {
        prepared.input =
            Some(prepare_input(adapter, kernel, input, sender, owner, generation).await?);
    } else if !input.direct
        && let Action::Message(content) = input.action
    {
        prepared.earlier = Some((
            input.conversation,
            EarlierMessage {
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
                origin: prepared.origin,
            },
        ));
    }
    Ok(prepared)
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
        let bytes =
            match super::health::call(kernel, generation, adapter.fetch(&attachment.locator))
                .await?
            {
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
        for attempt in 1..=CHANNEL_SEND_ATTEMPTS {
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
                        && attempt < CHANNEL_SEND_ATTEMPTS
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
        _ = stop.cancelled() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}
