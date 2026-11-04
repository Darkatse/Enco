//! A service turning a request away as too large: the kernel keeps the last answered request as
//! the measurement, puts a refusal on every target its combination serves while the conversation
//! still holds the refused plan, rejects a resend of it or a compaction that leaves nothing to
//! answer, and bounds the resends.
#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
use crate::support;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
use support::*;

/// The words the scripted service refuses with.
const REFUSAL: &str = "The input token count exceeds the maximum number of tokens allowed 120000.";

/// Records each composition. While the reply target carries a refusal it leaves out the oldest
/// Log message the refused plan carried, or, with `resend`, writes the refused plan again.
struct Recording {
    inputs: Mutex<Vec<ComposeInput>>,
    resend: bool,
}

impl Recording {
    fn new(resend: bool) -> Arc<Self> {
        Arc::new(Self {
            inputs: Mutex::new(vec![]),
            resend,
        })
    }

    fn inputs(&self) -> Vec<ComposeInput> {
        self.inputs.lock().unwrap().clone()
    }
}

#[async_trait]
impl Composer for Recording {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        self.inputs.lock().unwrap().push(input.clone());
        let mut plan = reply_plan(input);
        if let Some(overflowed) = &input.reply.overflowed
            && !self.resend
        {
            let oldest = overflowed.plan.items.iter().find_map(|item| match item {
                PlanItem::Log { pos, .. } => Some(*pos),
                PlanItem::Message { .. } => None,
            });
            plan.items.retain(|item| match item {
                CompositionItem::Log { pos, .. } => oldest.is_none_or(|oldest| *pos > oldest),
                CompositionItem::Message { .. } => true,
            });
        }
        Ok(Composition::Plan(plan))
    }
}

/// Summarizes instead of replying once the reply target carries a refusal, asking for less after
/// a summary was refused; records each composition.
struct Summarizing(Mutex<Vec<ComposeInput>>);

#[async_trait]
impl Composer for Summarizing {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        self.0.lock().unwrap().push(input.clone());
        let (Some(_), Some(compaction), Some(upto)) = (
            &input.reply.overflowed,
            &input.compaction,
            input.transcript.round_ends.last(),
        ) else {
            return Ok(Composition::Plan(reply_plan(input)));
        };
        // A refused summary carries no Log items; a refused reply does.
        let summary_refused = compaction.overflowed.as_ref().is_some_and(|overflowed| {
            overflowed
                .plan
                .items
                .iter()
                .all(|item| matches!(item, PlanItem::Message { .. }))
        });
        let text = if summary_refused {
            "Summarize briefly"
        } else {
            "Summarize the history"
        };
        Ok(Composition::Compact {
            upto: *upto,
            plan: CompositionPlan {
                items: vec![CompositionItem::Message {
                    role: Role::User,
                    text: text.into(),
                    sources: vec![],
                }],
                tools: vec![],
                max_output_tokens: Some(compaction.budget.max_output_tokens.get()),
                omitted: vec![],
            },
        })
    }
}

/// A kernel whose Runs end with their first Round, so no safe-mode Round follows a failure.
async fn start(
    root: &Path,
    provider: Arc<dyn Provider>,
    mut document: Value,
    composer: Arc<dyn Composer>,
) -> Kernel {
    document["run"] = json!({"max_rounds": 1});
    kernel_configured_composer(root, provider, document, vec![], composer, |_| {})
        .await
        .0
}

async fn send(kernel: &Kernel, text: &str) -> Vec<Entry> {
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).await.unwrap();
    kernel
        .submit(session.id, EventId::new(), text.into(), Vec::new())
        .await
        .unwrap();
    finish(&mut rx).await
}

/// A reply whose request the service measured at `input_tokens`.
fn measured(text: &str, input_tokens: u64) -> Result<Completion, Failure> {
    let mut completion = reply(text)?;
    completion.usage.input_tokens = input_tokens;
    Ok(completion)
}

fn refusal(counted: Option<u64>, accepted: Option<u64>) -> Result<Completion, CompleteError> {
    Err(CompleteError::Overflow(Overflow {
        counted,
        accepted,
        message: REFUSAL.into(),
    }))
}

/// Each Attempt's model and plan, in order.
fn attempts(entries: &[Entry]) -> Vec<(&str, ContentHash)> {
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::AttemptStarted { settings, plan, .. } => {
                Some((settings.model.as_str(), *plan))
            }
            _ => None,
        })
        .collect()
}

fn results(entries: &[Entry]) -> Vec<&AttemptResult> {
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::AttemptSettled { result, .. } => Some(result),
            _ => None,
        })
        .collect()
}

fn run_end(entries: &[Entry]) -> Option<&RunEnd> {
    match &entries.last()?.body {
        EntryBody::RunEnded { end, .. } => Some(end),
        _ => None,
    }
}

fn run_failure(entries: &[Entry]) -> Option<&Failure> {
    match run_end(entries)? {
        RunEnd::Failed { failure } => Some(failure),
        _ => None,
    }
}

fn plan_hash(plan: &ContextPlan) -> ContentHash {
    plan.hash().unwrap()
}

#[tokio::test(start_paused = true)]
async fn a_failed_reply_leaves_the_last_answered_request_as_the_measurement() {
    let dir = tempfile::tempdir().unwrap();
    let busy = Failure {
        code: code::PROVIDER_RATE_LIMITED.into(),
        message: "HTTP 429: resource exhausted".into(),
        retryable: true,
    };
    let provider = ScriptedProvider::new(vec![
        measured("first", 4321),
        Err(busy.clone()),
        Err(busy.clone()),
        Err(busy.clone()),
        reply("third"),
    ]);
    let composer = Recording::new(false);
    let kernel = start(dir.path(), provider, configuration(), composer.clone()).await;
    let first = send(&kernel, "first").await;
    let second = send(&kernel, "second").await;
    assert_eq!(run_failure(&second), Some(&busy));
    let refused = attempts(&second);
    assert_eq!(refused.len(), 3);
    send(&kernel, "third").await;
    let third = composer.inputs().pop().unwrap();
    let previous = plan_hash(third.previous_plan.as_ref().unwrap());
    assert_eq!(previous, attempts(&first)[0].1);
    assert_ne!(previous, refused[0].1);
    assert_eq!(third.reply.previous_usage.unwrap().input_tokens, 4321);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn retargeting_drops_the_measurement_another_model_took() {
    let dir = tempfile::tempdir().unwrap();
    let primary = ScriptedProvider::new(vec![
        measured("first", 4321),
        Err(Failure {
            code: "provider.payment_required".into(),
            message: "account returned 402".into(),
            retryable: false,
        }),
    ]);
    let backup = ScriptedProvider::new(vec![reply("from the backup")]);
    let provider = Arc::new(Models(BTreeMap::from([
        ("primary", primary as Arc<dyn Provider>),
        ("backup", backup as Arc<dyn Provider>),
    ])));
    let composer = Recording::new(false);
    let kernel = start(dir.path(), provider, candidates(), composer.clone()).await;
    let first = send(&kernel, "first").await;
    let second = send(&kernel, "second").await;
    assert_eq!(
        attempts(&second)
            .into_iter()
            .map(|(model, _)| model)
            .collect::<Vec<_>>(),
        ["primary", "backup"]
    );
    let inputs = composer.inputs();
    assert_eq!(inputs.len(), 3);
    let (on_primary, on_backup) = (&inputs[1], &inputs[2]);
    assert!(on_primary.reply.skipped.is_empty());
    assert_eq!(
        on_primary
            .reply
            .previous_usage
            .as_ref()
            .unwrap()
            .input_tokens,
        4321
    );
    assert!(!on_backup.reply.skipped.is_empty());
    assert_eq!(
        plan_hash(on_backup.previous_plan.as_ref().unwrap()),
        attempts(&first)[0].1
    );
    assert!(on_backup.reply.previous_usage.is_none());
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_overflow_resends_a_smaller_plan_to_the_same_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::answering(vec![
        measured("first", 4321).map_err(CompleteError::from),
        refusal(Some(150_000), Some(120_000)),
        reply("second").map_err(CompleteError::from),
    ]);
    let composer = Recording::new(false);
    let kernel = start(dir.path(), provider, configuration(), composer.clone()).await;
    send(&kernel, "first").await;
    let second = send(&kernel, "second").await;
    assert!(matches!(run_end(&second), Some(RunEnd::Completed)));
    assert!(matches!(
        results(&second).as_slice(),
        [
            AttemptResult::Overflow { .. },
            AttemptResult::Completed { .. }
        ]
    ));
    let sent = attempts(&second);
    assert_eq!(sent[0].0, sent[1].0);
    assert_ne!(sent[0].1, sent[1].1);
    let resend = composer.inputs().pop().unwrap();
    let overflowed = resend.reply.overflowed.unwrap();
    assert_eq!(plan_hash(&overflowed.plan), sent[0].1);
    assert_eq!(
        overflowed.overflow,
        Overflow {
            counted: Some(150_000),
            accepted: Some(120_000),
            message: REFUSAL.into(),
        }
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn returning_the_refused_plan_breaks_the_composer_contract() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::answering(vec![refusal(None, None)]);
    let composer = Recording::new(true);
    let kernel = start(
        dir.path(),
        provider.clone(),
        configuration(),
        composer.clone(),
    )
    .await;
    let log = send(&kernel, "hello").await;
    assert_eq!(run_failure(&log).unwrap().code, code::PLUGIN_CONTRACT);
    assert_eq!(composer.inputs().len(), 2);
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_service_that_keeps_refusing_ends_the_round_after_three_resends() {
    let dir = tempfile::tempdir().unwrap();
    let primary = ScriptedProvider::answering(vec![
        reply("first").map_err(CompleteError::from),
        reply("second").map_err(CompleteError::from),
        refusal(None, Some(120_000)),
        refusal(None, Some(120_000)),
        refusal(None, Some(120_000)),
        refusal(None, Some(120_000)),
    ]);
    let backup = ScriptedProvider::new(vec![]);
    let provider = Arc::new(Models(BTreeMap::from([
        ("primary", primary as Arc<dyn Provider>),
        ("backup", backup.clone() as Arc<dyn Provider>),
    ])));
    let kernel = start(dir.path(), provider, candidates(), Recording::new(false)).await;
    send(&kernel, "first").await;
    send(&kernel, "second").await;
    let log = send(&kernel, "third").await;
    let sent = attempts(&log);
    assert_eq!(
        sent.iter().map(|(model, _)| *model).collect::<Vec<_>>(),
        ["primary"; 4]
    );
    let failure = run_failure(&log).unwrap();
    assert_eq!(failure.code, code::CONTEXT_OVERFLOW);
    assert!(failure.message.contains(REFUSAL), "{}", failure.message);
    assert!(backup.requests.lock().unwrap().is_empty());
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_refusal_reaches_every_target_its_combination_serves() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::answering(vec![
        reply("first").map_err(CompleteError::from),
        refusal(Some(150_000), Some(120_000)),
        refusal(None, None),
        reply("The owner said first.").map_err(CompleteError::from),
        reply("second").map_err(CompleteError::from),
    ]);
    let composer = Arc::new(Summarizing(Mutex::new(vec![])));
    // The reply and compaction endpoints share their call parameters, so one combination.
    let kernel = start(dir.path(), provider, configuration(), composer.clone()).await;
    send(&kernel, "first").await;
    let log = send(&kernel, "second").await;
    assert!(matches!(run_end(&log), Some(RunEnd::Completed)));
    let sent = attempts(&log);
    let refused = |input: &ComposeInput| {
        let target = |overflowed: Option<&Overflowed>| overflowed.map(|o| plan_hash(&o.plan));
        (
            target(input.reply.overflowed.as_ref()),
            target(input.compaction.as_ref().unwrap().overflowed.as_ref()),
        )
    };
    let inputs = composer.0.lock().unwrap().clone();
    // The first Run's reply, then: the refused reply, the refused summary, the shorter summary
    // and the reply after it.
    assert_eq!(inputs.len(), 5);
    assert_eq!(refused(&inputs[2]), (Some(sent[0].1), Some(sent[0].1)));
    assert_eq!(refused(&inputs[3]), (Some(sent[1].1), Some(sent[1].1)));
    // The compaction summarized what the refused reply carried; the refused summary still
    // measures this service, so the reply after it keeps to that request's share.
    assert_eq!(refused(&inputs[4]), (Some(sent[1].1), Some(sent[1].1)));
    kernel.shutdown().await.unwrap();
}

/// Summarizes up to the latest Round end whenever one exists.
struct Exhausting;

#[async_trait]
impl Composer for Exhausting {
    async fn compose(
        &self,
        _: &ComposerOptions,
        input: &ComposeInput,
    ) -> Result<Composition, Failure> {
        let (Some(compaction), Some(upto)) =
            (&input.compaction, input.transcript.round_ends.last())
        else {
            return Ok(Composition::Plan(reply_plan(input)));
        };
        Ok(Composition::Compact {
            upto: *upto,
            plan: CompositionPlan {
                items: vec![CompositionItem::Message {
                    role: Role::User,
                    text: "Summarize the history".into(),
                    sources: vec![],
                }],
                tools: vec![],
                max_output_tokens: Some(compaction.budget.max_output_tokens.get()),
                omitted: vec![],
            },
        })
    }
}

#[tokio::test]
async fn a_compaction_that_leaves_nothing_to_answer_breaks_the_composer_contract() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![calls(vec![call("fs_list", r#"{"path":"."}"#)])]);
    let mut document = configuration();
    document["run"] = json!({"max_rounds": 2});
    let kernel = kernel_configured_composer(
        dir.path(),
        provider.clone(),
        document,
        vec![],
        Arc::new(Exhausting),
        |_| {},
    )
    .await
    .0;
    // The second Round starts with nothing after the first Round's end.
    let log = send(&kernel, "list the files").await;
    assert_eq!(run_failure(&log).unwrap().code, code::PLUGIN_CONTRACT);
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    kernel.shutdown().await.unwrap();
}
