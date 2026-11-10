//! All mapper call results report against the mapper code held by the connection.
use super::{ChannelCallError, ChannelError};
use enco_core::CodeRef;
use enco_kernel::{Kernel, Verdict};
use std::future::Future;

pub(super) async fn call<T>(
    kernel: &Kernel,
    code: &CodeRef,
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
            .report(code, verdict, None)
            .await
            .map_err(enco_kernel::KernelError::from)?;
    }
    result.map_err(ChannelError::from)
}
