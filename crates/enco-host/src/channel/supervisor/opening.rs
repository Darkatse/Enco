//! Cancellable account identification; only the supervisor admits an identified account.
use super::*;
use enco_kernel::{ChannelBackoff, ChannelSettings, Export, Store};

/// One connection's identification in progress, shared by its slot and its task.
pub(super) struct Opening {
    /// The generation that identifies the account and then serves it.
    pub generation: GenerationId,
    /// The latest retryable failure, shown while the next attempt waits.
    pub retrying: Mutex<Option<ChannelFailure>>,
    stop: CancellationToken,
}

/// An identification's end: its connection, the opening that ran it, and the account it reached,
/// or none once stopped.
pub(super) type Opened = (
    String,
    Arc<Opening>,
    Result<Option<Arc<dyn Adapter>>, ChannelError>,
);

impl Opening {
    /// Identify the connection's account as a task of `openings`, retrying retryable failures
    /// with backoff until it succeeds, fails for good or is stopped.
    pub fn start(
        openings: &mut JoinSet<Opened>,
        name: String,
        kernel: Arc<Kernel>,
        store: Arc<dyn Store>,
        settings: ChannelSettings,
        export: Export<dyn enco_kernel::Channel>,
        stop: CancellationToken,
    ) -> Arc<Self> {
        let opening = Arc::new(Self {
            generation: export.generation,
            retrying: Mutex::new(None),
            stop,
        });
        let task = opening.clone();
        openings.spawn(async move {
            let result = task.identify(&kernel, store, &settings, export).await;
            (name, task, result)
        });
        opening
    }

    /// Signal the identification to stop; its end still arrives through the set.
    pub fn stop(&self) {
        self.stop.cancel();
    }

    async fn identify(
        &self,
        kernel: &Kernel,
        store: Arc<dyn Store>,
        settings: &ChannelSettings,
        export: Export<dyn enco_kernel::Channel>,
    ) -> Result<Option<Arc<dyn Adapter>>, ChannelError> {
        let mut backoff = ChannelBackoff::default();
        loop {
            let opened = export
                .adapter
                .open(settings, export.generation, store.clone());
            let error = tokio::select! {
                biased;
                () = self.stop.cancelled() => return Ok(None),
                result = health::call(kernel, export.generation, opened) => match result {
                    Ok(adapter) => return Ok(Some(adapter)),
                    Err(error) => error,
                },
            };
            let ChannelError::Protocol(ChannelCallError::Protocol(fault)) = &error else {
                return Err(error);
            };
            if fault.fatal || !fault.failure.retryable {
                return Err(error);
            }
            let delay = backoff.next(fault.retry_after);
            *lock(&self.retrying) = Some(ChannelFailure::from(&error));
            tokio::select! {
                biased;
                () = self.stop.cancelled() => return Ok(None),
                () = tokio::time::sleep(delay) => {}
            }
        }
    }
}
