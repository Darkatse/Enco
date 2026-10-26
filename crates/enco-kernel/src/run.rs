use crate::{session::SessionActor, *};
use enco_core::*;
use tokio_util::sync::CancellationToken;

pub(crate) enum RoundError {
    /// An owner could not commit its state; stop without claiming that the Round settled.
    Stopped(KernelError),
    Cancelled,
    Failed(Failure),
}

impl From<StoreError> for RoundError {
    fn from(error: StoreError) -> Self {
        Self::Stopped(error.into())
    }
}
impl From<RegistryError> for RoundError {
    fn from(error: RegistryError) -> Self {
        Self::Stopped(error.into())
    }
}

pub(crate) fn failed(code: &str, message: impl Into<String>) -> RoundError {
    RoundError::Failed(Failure {
        code: code.into(),
        message: message.into(),
        retryable: false,
    })
}

pub(crate) fn cancelled(token: &CancellationToken) -> Result<(), RoundError> {
    if token.is_cancelled() {
        Err(RoundError::Cancelled)
    } else {
        Ok(())
    }
}

impl SessionActor {
    pub(crate) async fn run(&mut self, token: CancellationToken) -> Result<(), KernelError> {
        let run = RunId::new();
        // The first Round accepts RunStarted and its Inbox inputs in one Commit.
        let mut prefix = vec![EntryBody::RunStarted { run }];
        let mut ending = RunEnd::BudgetExhausted;
        let max_rounds = self.deps.registry.view().config.kernel.max_rounds_per_run;
        let mut pending = self.deps.store.pending(self.session.id).await?;
        for _ in 0..max_rounds.get() {
            // The owner went back: the Run stops at this boundary, and the backtrack follows it.
            if pending.iter().any(crate::transcript::is_backtrack) {
                if !prefix.is_empty() {
                    // Nothing of this Run is recorded yet, so it does not start at all.
                    return Ok(());
                }
                ending = RunEnd::Cancelled;
                break;
            }
            let inputs = std::mem::take(&mut pending);
            let (end, safe_mode) = self
                .round(run, &token, std::mem::take(&mut prefix), inputs)
                .await?;
            pending = self.deps.store.pending(self.session.id).await?;
            // How the Run ends if it stops after this Round, and whether it stops now.
            let stop;
            (ending, stop) = match end {
                RoundEnd::Replied
                    if pending.is_empty()
                        || pending.iter().any(crate::transcript::is_backtrack) =>
                {
                    (RunEnd::Completed, true)
                }
                RoundEnd::Replied | RoundEnd::ToolsSettled => (RunEnd::BudgetExhausted, false),
                // A failure on the normal path gets the next Round in safe mode, if one remains.
                RoundEnd::Failed { failure, .. } => (RunEnd::Failed { failure }, safe_mode),
                RoundEnd::Cancelled => (RunEnd::Cancelled, true),
                RoundEnd::Interrupted => (RunEnd::Interrupted, true),
            };
            if stop {
                break;
            }
        }
        self.commit(vec![EntryBody::RunEnded { run, end: ending }], None)
            .await?;
        Ok(())
    }
}
