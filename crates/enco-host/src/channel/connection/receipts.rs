//! Accepted command receipts wait for protocol acknowledgments outside the connection owner.
use super::*;

impl Connection {
    pub(super) fn answer(&mut self, acknowledged: tokio::sync::oneshot::Receiver<()>) {
        if self.receipts.is_empty() {
            return;
        }
        let receipts = std::mem::take(&mut self.receipts);
        let adapter = self.adapter.clone();
        let kernel = self.kernel.clone();
        let generation = self.generation;
        let stop = self.stop.clone();
        let pacing = self.pacing.clone();
        self.replies.spawn(async move {
            if acknowledged.await.is_err() {
                tracing::warn!("receipt acknowledgment task stopped before answers could be sent");
                return Ok(());
            }
            for receipt in receipts {
                match receipt {
                    Receipt::Answer { handle, text } => {
                        match health::call(&kernel, generation, adapter.respond(&handle, text.as_deref())).await {
                            Ok(()) => {}
                            Err(error @ ChannelError::Protocol(ChannelCallError::Protocol(_))) => tracing::warn!(%error, "channel answer failed"),
                            Err(error) => return Err(error),
                        }
                    }
                    Receipt::Say { target, text } => {
                        let sent = transport::send(adapter.clone(), kernel.clone(), generation, target, Outgoing { text, file: None }, stop.clone(), pacing.clone()).await?;
                        if let Some(error) = sent.fatal { return Err(ChannelCallError::from(error).into()); }
                        if !matches!(sent.outcome, Settlement::Ok) { tracing::warn!(outcome = ?sent.outcome, "command receipt was not delivered"); }
                    }
                }
            }
            Ok(())
        });
    }
}
