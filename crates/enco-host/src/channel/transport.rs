use super::{Action, Adapter, Attachment, AttachmentContent, ChannelError, Fault, MessageContent};
use enco_core::*;
use enco_kernel::{ImageError, Kernel};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::limits::{CHANNEL_MAX_BACKOFF, CHANNEL_SEND_ATTEMPTS};

pub(super) struct Batch {
    pub updates: Vec<Prepared>,
    pub committed: oneshot::Sender<Value>,
}

pub(super) struct Prepared {
    pub protocol: Value,
    pub owner: Option<OwnerInput>,
}

pub(super) struct OwnerInput {
    pub conversation: String,
    pub action: Action<Image>,
}

pub(super) async fn poll(
    adapter: Arc<dyn Adapter>,
    kernel: Arc<Kernel>,
    owner: String,
    mut protocol: Value,
    batches: mpsc::Sender<Batch>,
    stop: CancellationToken,
) -> Result<(), ChannelError> {
    let mut delay = Duration::from_secs(1);
    loop {
        let result = tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            result = prepare(adapter.as_ref(), &kernel, &owner, protocol.clone()) => result,
        };
        let (updates, failure) = result?;
        if !updates.is_empty() {
            let (committed, next) = oneshot::channel();
            tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                result = batches.send(Batch { updates, committed }) => {
                    result.map_err(|_| ChannelError::Task("connection stopped".into()))?;
                }
            }
            protocol = tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                result = next => result.map_err(|_| ChannelError::Task("update was not accepted".into()))?,
            };
        }
        if let Some(error) = failure {
            if error.fatal || !error.failure.retryable {
                return Err(error.into());
            }
            tracing::warn!(%error, "channel polling will retry");
            if wait(error.retry_after.unwrap_or(delay), &stop).await {
                return Ok(());
            }
            delay = (delay * 2).min(CHANNEL_MAX_BACKOFF);
        } else {
            delay = Duration::from_secs(1);
        }
    }
}

/// A protocol fault stops preparation for a retry; the updates prepared before it are committed.
async fn prepare(
    adapter: &dyn Adapter,
    kernel: &Kernel,
    owner: &str,
    protocol: Value,
) -> Result<(Vec<Prepared>, Option<Fault>), ChannelError> {
    let updates = match adapter.poll(protocol).await {
        Ok(updates) => updates,
        Err(failure) => return Ok((Vec::new(), Some(failure))),
    };
    let mut ready = Vec::new();
    for update in updates {
        let owner = match update.input {
            Some(input) if input.direct && input.sender.as_deref() == Some(owner) => {
                match input.action {
                    Some(action) => match prepare_action(adapter, kernel, action).await? {
                        Ok(action) => Some(OwnerInput {
                            conversation: input.conversation,
                            action,
                        }),
                        Err(fault) => return Ok((ready, Some(fault))),
                    },
                    None => None,
                }
            }
            Some(input) => {
                tracing::info!(
                    sender_id = input.sender.as_deref().unwrap_or("unknown"),
                    chat_id = %input.conversation,
                    chat_type = %input.conversation_type,
                    "channel input ignored"
                );
                None
            }
            None => None,
        };
        ready.push(Prepared {
            protocol: update.protocol,
            owner,
        });
    }
    Ok((ready, None))
}

/// A protocol fault comes back inside for a retry; a store failure stops the channel.
async fn prepare_action(
    adapter: &dyn Adapter,
    kernel: &Kernel,
    action: Action<Attachment>,
) -> Result<Result<Action<Image>, Fault>, ChannelError> {
    let message = match action {
        Action::Message(message) => message,
        Action::Sessions => return Ok(Ok(Action::Sessions)),
        Action::SelectSession(name) => return Ok(Ok(Action::SelectSession(name))),
        Action::Cancel => return Ok(Ok(Action::Cancel)),
    };
    let mut prepared = MessageContent {
        text: message.text,
        images: Vec::new(),
        unviewable: message.unviewable,
    };
    for attachment in message.images {
        let bytes = match adapter.fetch(&attachment.locator).await {
            Ok(AttachmentContent::Image(bytes)) => bytes,
            Ok(AttachmentContent::Unavailable(reason)) => {
                prepared.unviewable.push(Unviewable {
                    what: attachment.what,
                    reason: UnviewableReason::Unavailable(reason),
                });
                continue;
            }
            Err(fault) => return Ok(Err(fault)),
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
    Ok(Ok(Action::Message(prepared)))
}

pub(super) struct Sent {
    pub outcome: Settlement,
    pub fatal: Option<Fault>,
}

/// Each successful part leaves this loop permanently; unknown effects are never retried.
pub(super) async fn send(
    adapter: Arc<dyn Adapter>,
    target: String,
    text: String,
    stop: CancellationToken,
) -> Sent {
    let parts = adapter.split(&text);
    for (part, text) in parts.iter().enumerate() {
        let mut delay = Duration::from_secs(1);
        for attempt in 1..=CHANNEL_SEND_ATTEMPTS {
            // A send future has effects: await it even after cancellation.
            match adapter.send(&target, text).await {
                Ok(()) => break,
                Err(mut error) => {
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
                    return Sent {
                        outcome,
                        fatal: error.fatal.then_some(error),
                    };
                }
            }
        }
        if stop.is_cancelled() && part + 1 < parts.len() {
            return Sent {
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
                fatal: None,
            };
        }
    }
    Sent {
        outcome: Settlement::Ok,
        fatal: None,
    }
}

async fn wait(delay: Duration, stop: &CancellationToken) -> bool {
    tokio::select! {
        _ = stop.cancelled() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}
