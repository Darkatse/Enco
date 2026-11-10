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
        if command_end(&pending).is_some() {
            // Inter-Run commands are consumed by the actor without starting a Run.
            return Ok(());
        }
        for _ in 0..max_rounds.get() {
            let inputs = self.at_boundary(std::mem::take(&mut pending)).await?;
            let (end, safe_mode) = self
                .round(run, &token, std::mem::take(&mut prefix), inputs)
                .await?;
            pending = self.deps.store.pending(self.session.id).await?;
            // A request that arrives during the Run is for the next one; only the first Round
            // consumes the requests waiting when the Run began.
            pending.retain(|event| !matches!(event.body, EventBody::Compact));
            if let Some(end) = command_end(&pending) {
                ending = end;
                break;
            }
            // Only an input keeps the Run going; a command acting on the Session waits for the
            // next boundary, or is consumed once the Session is idle.
            let inputs_wait = pending.iter().any(|event| !event.body.is_command());
            // How the Run ends if it stops after this Round, and whether it stops now.
            let stop;
            (ending, stop) = match end {
                RoundEnd::Replied | RoundEnd::ReplySkipped if !inputs_wait => {
                    (RunEnd::Completed, true)
                }
                RoundEnd::Replied | RoundEnd::ReplySkipped | RoundEnd::ToolsSettled => {
                    (RunEnd::BudgetExhausted, false)
                }
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

impl SessionActor {
    /// Consume the commands acting on the Session, so the Round reads it with them applied, and
    /// return the rest.
    async fn at_boundary(&mut self, pending: Vec<Event>) -> Result<Vec<Event>, KernelError> {
        let (commands, rest): (Vec<_>, Vec<_>) = pending
            .into_iter()
            .partition(|event| event.body.command_on() == Some(CommandOn::Session));
        if !commands.is_empty() {
            let consumed = commands
                .into_iter()
                .map(|event| EntryBody::EventConsumed { event })
                .collect();
            self.commit(consumed, None).await?;
        }
        Ok(rest)
    }
}

fn command_end(pending: &[Event]) -> Option<RunEnd> {
    pending.iter().rev().find_map(|event| match event.body {
        EventBody::Halt(..) => Some(RunEnd::Halted),
        EventBody::Backtrack { .. } => Some(RunEnd::Backtracked),
        _ => None,
    })
}
