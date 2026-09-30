use async_trait::async_trait;
use enco_core::*;
use tokio_util::sync::CancellationToken;

/// An executable capability; dispatch is independent of tool identity.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Model-facing contract, frozen into the Round's plan.
    fn spec(&self) -> ToolSpec;
    /// Code implementing the invocation.
    fn code(&self) -> CodeRef;
    /// Execute after dispatch checks disclosure, argument names and whether the call may start.
    /// Respond to cancellation and wait until started work is quiescent before returning.
    /// Pageable results should fit ctx.result_budget and explain how to continue.
    async fn call(
        &self,
        ctx: CallContext,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> Outcome;
}

/// Invocation identity, cooperative cancellation and the inline result budget.
pub struct CallContext {
    /// Session which proposed the call.
    pub session: SessionId,
    /// Host-assigned identity used for durable settlement.
    pub call: CallId,
    /// Child of the Run cancellation token.
    pub cancel: CancellationToken,
    /// Maximum inline result size in bytes, including any continuation instructions.
    pub result_budget: usize,
}
