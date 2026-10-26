//! All mapper call results report against the generation held by the connection.
use super::{ChannelCallError, ChannelError};
use enco_core::GenerationId;
use enco_kernel::{Kernel, Verdict};
use std::future::Future;

pub(super) async fn call<T>(
    kernel: &Kernel,
    generation: GenerationId,
    operation: impl Future<Output = Result<T, ChannelCallError>>,
) -> Result<T, ChannelError> {
    let result = operation.await;
    let verdict = match &result {
        Ok(_) => Some(Verdict::Ok),
        Err(ChannelCallError::Protocol(error)) => Some(Verdict::Failed(error.failure.clone())),
        Err(ChannelCallError::Store(_)) => None,
    };
    if let Some(verdict) = verdict {
        kernel
            .registry()
            .report(generation, verdict, None)
            .await
            .map_err(enco_kernel::KernelError::from)?;
    }
    result.map_err(ChannelError::from)
}
