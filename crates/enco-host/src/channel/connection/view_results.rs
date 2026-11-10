//! View request outcomes commit protocol identities, retry known refusals, and forget gone routes.
use super::*;
use activity::{Card, Completed, Operation};
use enco_kernel::{ChannelActivityUpdate, ChannelBackoff};
use tokio::time::Instant;

impl Connection {
    pub(super) async fn activity_completed(
        &mut self,
        completed: Completed,
    ) -> Result<(), ChannelError> {
        let Completed { request, result } = completed;
        let activity::Request { target, operation } = request;
        match &operation {
            Operation::Open { session, .. } | Operation::Close { session } => {
                self.views.conversation_busy.remove(session);
            }
            Operation::Activity { run, .. } => {
                self.views.activity_busy.remove(run);
            }
            Operation::Presence { run, .. } => {
                self.views.presence_busy.remove(run);
            }
        }
        let sent = match result {
            Ok(sent) => {
                self.views.backoff = ChannelBackoff::default();
                sent
            }
            Err(ChannelError::Protocol(ChannelCallError::Protocol(fault))) => {
                tracing::warn!(error = %fault, "channel view request failed");
                // A card or presence that finds its conversation gone forgets it. Opening names the
                // parent's conversation only as the place to open beside, so its failure says
                // nothing about that conversation; closing removes the route either way.
                if fault.failure.code == code::CHANNEL_CONVERSATION_GONE
                    && matches!(
                        operation,
                        Operation::Activity { .. } | Operation::Presence { .. }
                    )
                {
                    return self.conversation_gone(&target).await;
                }
                if !fault.unknown && !fault.fatal && fault.failure.retryable {
                    // The protocol did not carry the request out; all views wait, then try again.
                    let retry_at = Instant::now() + self.views.backoff.next(fault.retry_after);
                    self.views.retry_at = Some(
                        self.views
                            .retry_at
                            .map_or(retry_at, |current| current.max(retry_at)),
                    );
                    return Ok(());
                }
                if matches!(
                    operation,
                    Operation::Presence { .. }
                        | Operation::Activity {
                            shown: Some(_),
                            update: ChannelActivityUpdate::View(_),
                            ..
                        }
                ) {
                    return Ok(());
                }
                None
            }
            Err(error) => return Err(error),
        };
        self.record_view(operation, target, sent).await
    }

    async fn record_view(
        &mut self,
        operation: Operation,
        target: String,
        sent: Option<String>,
    ) -> Result<(), ChannelError> {
        match operation {
            Operation::Open { session, .. } => {
                let mut state = self.state.clone();
                let creation = match sent {
                    Some(conversation) => {
                        state.chats.insert(conversation.clone(), session);
                        Appearance::Shown(conversation)
                    }
                    None => Appearance::Forgone,
                };
                state.children.insert(session, creation);
                self.commit(state, None).await?;
            }
            Operation::Close { session } => {
                let mut state = self.state.clone();
                state.children.remove(&session);
                if state.chats.get(&target) == Some(&session) {
                    state.chats.remove(&target);
                }
                self.commit(state, None).await?;
            }
            Operation::Activity {
                session,
                run,
                replies_here,
                shown,
                update,
            } => match update {
                ChannelActivityUpdate::View(_) if shown.is_none() => {
                    let mut state = self.state.clone();
                    state.activity.insert(
                        run,
                        Card {
                            session,
                            target,
                            replies_here,
                            creation: sent.map_or(Appearance::Forgone, Appearance::Shown),
                        },
                    );
                    self.commit(state, None).await?;
                }
                ChannelActivityUpdate::View(_) => {}
                ChannelActivityUpdate::Remove
                    if self
                        .observations
                        .get(&session)
                        .and_then(|observation| observation.runs.get(&run))
                        .is_some_and(|run| run.ended.is_none()) =>
                {
                    self.clear_card(run).await?;
                }
                ChannelActivityUpdate::Epilogue(_) | ChannelActivityUpdate::Remove => {
                    self.finish_view(session, run).await?;
                }
            },
            Operation::Presence { .. } => {}
        }
        Ok(())
    }

    async fn conversation_gone(&mut self, target: &str) -> Result<(), ChannelError> {
        let mut state = self.state.clone();
        state.forget(target);
        self.commit(state, None).await
    }
}
