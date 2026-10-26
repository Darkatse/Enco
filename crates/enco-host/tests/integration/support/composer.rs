use super::*;

/// The factory composer's reserved name, as the host supplies it.
pub const COMPOSER: &str = "composer";
pub const COMPOSER_BYTES: &[u8] = b"scripted-composer";
pub const COMPOSER_ID: &str = "01M5TKQCBXJHD1X8JQPFDVJDDJ";

pub struct TestComposer;

pub fn reply_plan(input: &ComposeInput) -> CompositionPlan {
    let mut items = Vec::new();
    if let Some(summary) = &input.transcript.summary {
        items.push(CompositionItem::Message {
            role: Role::System,
            text: summary.clone(),
            sources: vec![],
        });
    }
    items.extend(
        input
            .transcript
            .items
            .iter()
            .map(|item| CompositionItem::Log {
                pos: item.pos,
                images: input.reply.accepts.contains(&Modality::Image),
            }),
    );
    CompositionPlan {
        items,
        tools: input.tools.iter().map(|(id, _)| id.clone()).collect(),
        max_output_tokens: Some(input.reply.budget.max_output_tokens.get()),
        omitted: vec![],
    }
}

#[async_trait]
impl Composer for TestComposer {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        Ok(Composition::Plan(reply_plan(input)))
    }
}

/// Summarizes after each tool result; a second unsummarized result exhausts the scripted window.
pub struct HistoryComposer;

#[async_trait]
impl Composer for HistoryComposer {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        let results = input
            .transcript
            .items
            .iter()
            .flat_map(|item| &item.message.parts)
            .filter(|part| matches!(part, Part::ToolResult(_)))
            .count();
        if results > 0
            && input.compactions_left > 0
            && let (Some(budget), Some(upto)) =
                (input.compaction, input.transcript.round_ends.last())
        {
            return Ok(Composition::Compact {
                upto: *upto,
                plan: CompositionPlan {
                    items: vec![CompositionItem::Message {
                        role: Role::User,
                        text: "Summarize the scripted history".into(),
                        sources: vec![],
                    }],
                    tools: vec![],
                    max_output_tokens: Some(budget.max_output_tokens.get()),
                    omitted: vec![],
                },
            });
        }
        if results > 1 {
            return Err(Failure {
                code: code::CONTEXT_OVERFLOW.into(),
                message: "scripted window exhausted".into(),
                retryable: false,
            });
        }
        Ok(Composition::Plan(reply_plan(input)))
    }
}

pub fn composer_loaded(adapter: Arc<dyn Composer>) -> Loaded {
    Loaded {
        lifecycle: Arc::new(ScriptedLifecycle(Ok(()))),
        summary: "scripted composer".into(),
        config_schema: serde_json::json!({}),
        credentials: vec![],
        unavailable: None,
        composer: Some(LoadedComposer {
            schema: InputSchema::empty(),
            adapter,
        }),
        completion: None,
        embedding: None,
        decision: None,
        channel: None,
        tools: None,
    }
}
