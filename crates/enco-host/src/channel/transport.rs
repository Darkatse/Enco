use super::{Adapter, ChannelError, Fault, Update};
use enco_core::*;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::limits::{CHANNEL_MAX_BACKOFF, CHANNEL_SEND_ATTEMPTS};

pub(super) struct Batch {
    pub updates: Vec<Update>,
    pub committed: oneshot::Sender<Value>,
}

pub(super) async fn poll(
    adapter: Arc<dyn Adapter>,
    mut protocol: Value,
    batches: mpsc::Sender<Batch>,
    stop: CancellationToken,
) -> Result<(), ChannelError> {
    let mut delay = Duration::from_secs(1);
    loop {
        let result = tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            result = adapter.poll(protocol.clone()) => result,
        };
        match result {
            Ok(updates) => {
                delay = Duration::from_secs(1);
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
            Err(error) => {
                if error.fatal || !error.failure.retryable {
                    return Err(error.into());
                }
                tracing::warn!(%error, "channel polling will retry");
                if wait(error.retry_after.unwrap_or(delay), &stop).await {
                    return Ok(());
                }
                delay = (delay * 2).min(CHANNEL_MAX_BACKOFF);
            }
        }
    }
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
