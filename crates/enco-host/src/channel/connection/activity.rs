//! Ephemeral activity rendering follows Log facts and never participates in delivery settlement.
use super::*;

pub(super) async fn follow(
    kernel: Arc<Kernel>,
    adapter: Arc<dyn Adapter>,
    generation: GenerationId,
    session: SessionId,
    stop: CancellationToken,
) -> Result<(), ChannelError> {
    let mut entries = kernel.subscribe(session)?;
    let identity = adapter.identity();
    let mut cursor = None;
    let mut running = false;
    let mut target = None;
    let mut ticks = tokio::time::interval(crate::limits::CHANNEL_TYPING_INTERVAL);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        for entry in kernel.log(session, cursor).await? {
            cursor = Some(entry.pos);
            match entry.body {
                EntryBody::RunStarted { .. } => running = true,
                EntryBody::RunEnded { .. } => running = false,
                EntryBody::EventConsumed { event } => {
                    projection::route(&mut target, &identity, event.source)
                }
                _ => {}
            }
        }
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            result = entries.recv() => match result {
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {},
                Err(broadcast::error::RecvError::Closed) => return Err(ChannelError::Task(format!("Session {session} activity subscription closed"))),
            },
            _ = ticks.tick(), if running && target.is_some() => {
                if let Some(target) = &target && let Err(error) = health::call(&kernel, generation, adapter.typing(target)).await {
                    tracing::warn!(%error, "channel activity notification failed");
                }
            }
        }
    }
}
