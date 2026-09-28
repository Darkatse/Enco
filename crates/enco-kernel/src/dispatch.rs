use crate::{session::SessionActor, snapshot::Snapshot, *};
use enco_core::*;
use tokio_util::sync::CancellationToken;

pub(crate) fn render_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { value } => match value {
            serde_json::Value::String(s) => s.clone(),
            _ => format!("{value:#}"),
        },
        Outcome::Failed { failure } => format!("error [{}]: {}", failure.code, failure.message),
        Outcome::Unknown { failure } => format!(
            "outcome unknown [{}]: {}. The action may already have taken effect; check the current state before retrying.",
            failure.code, failure.message
        ),
    }
}

impl SessionActor {
    pub(crate) async fn settle_without_start(
        &mut self,
        call: &ToolCall,
        failure: Failure,
    ) -> Result<(), StoreError> {
        self.settle(call.id, Outcome::Failed { failure }).await
    }

    async fn settle(&mut self, call: CallId, outcome: Outcome) -> Result<(), StoreError> {
        let mut content = render_text(&outcome);
        let mut full = None;
        if content.len() > limits::TOOL_RESULT_INLINE_BYTES {
            let hash = self.deps.store.put_blob(content.as_bytes()).await?;
            let size = content.len();
            content.truncate(content.floor_char_boundary(limits::TOOL_RESULT_PREVIEW_BYTES));
            let path = self.deps.store.blob_path(&hash);
            content.push_str(&format!(
                "\n[truncated: {size} bytes. Full result: {}. \
                 Read it with fs_read using offset and limit.]",
                path.display(),
            ));
            full = Some(hash);
        }
        self.commit(
            vec![EntryBody::ToolCallSettled {
                call,
                outcome,
                content,
                full,
            }],
            vec![],
        )
        .await
    }

    pub(crate) async fn dispatch(
        &mut self,
        round: RoundId,
        snapshot: &Snapshot,
        plan: &ContextPlan,
        call: &ToolCall,
        token: &CancellationToken,
    ) -> Result<(), StoreError> {
        if token.is_cancelled() {
            return self
                .settle_without_start(
                    call,
                    Failure {
                        code: code::CANCELLED.into(),
                        message: "not executed: the run was cancelled before this call".into(),
                        retryable: false,
                    },
                )
                .await;
        }
        let tool = plan
            .tools
            .iter()
            .find(|(_, spec)| spec.name == call.name)
            .and_then(|(id, _)| snapshot.tools.iter().find(|t| t.id == *id));
        let Some(tool) = tool else {
            return self
                .settle_without_start(
                    call,
                    Failure {
                        code: code::TOOL_UNAVAILABLE.into(),
                        message: format!("tool `{}` is not available in this Round", call.name),
                        retryable: false,
                    },
                )
                .await;
        };
        let args = match Arguments::parse(&call.arguments) {
            Arguments::Object(args) => args,
            Arguments::Invalid { raw } => {
                return self
                    .settle_without_start(
                        call,
                        Failure {
                            code: code::TOOL_INVALID_ARGUMENTS.into(),
                            message: format!(
                                "arguments must be a JSON object; received: {}",
                                raw.chars().take(200).collect::<String>()
                            ),
                            retryable: false,
                        },
                    )
                    .await;
            }
        };
        if let Err(failure) = tool.spec.check_argument_names(&args) {
            return self.settle_without_start(call, failure).await;
        }
        self.commit(
            vec![EntryBody::ToolCallStarted {
                round,
                call: call.id,
                capability: tool.id.clone(),
                code: tool.code.clone(),
                effect: tool.spec.effect,
            }],
            vec![],
        )
        .await?;
        let outcome = tool
            .tool
            .call(
                CallContext {
                    session: self.session.id,
                    call: call.id,
                    cancel: token.child_token(),
                },
                args,
            )
            .await;
        self.settle(call.id, outcome).await
    }
}
