use super::*;
use crate::child_slots::ChildPermit;

impl SessionActor {
    pub(super) async fn child_permit(&self) -> Result<Option<ChildPermit>, KernelError> {
        let waiting = self.deps.slots.queue(self.session.id);
        let mut views = self.deps.registry.subscribe();
        loop {
            let changed = self.deps.slots.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let wake = self.handle.wake.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            // Archiving writes a halt, so this check also covers it.
            let pending = self.deps.store.pending(self.session.id).await?;
            if pending.iter().any(|event| event.body.supersedes_inputs())
                || self.deps.shutdown.is_cancelled()
            {
                return Ok(None);
            }
            let limit = self
                .deps
                .registry
                .view()
                .config
                .kernel
                .max_concurrent_children
                .get();
            if let Some(permit) = waiting.admit(limit) {
                return Ok(Some(permit));
            }
            tokio::select! {
                () = &mut changed => {},
                () = &mut wake => {},
                result = views.changed() => result.map_err(|error| KernelError::TaskFailed(error.to_string()))?,
                () = self.deps.shutdown.cancelled() => return Ok(None),
            }
        }
    }
}
