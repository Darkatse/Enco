#![expect(clippy::unwrap_used, reason = "test helpers fail by panicking")]
mod support;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::*;

struct Models(BTreeMap<&'static str, Arc<dyn Provider>>);

#[async_trait]
impl Provider for Models {
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        self.0[settings.model.as_str()]
            .complete(settings, request)
            .await
    }
}

/// Notes, on Tokio's clock, when each call reaches the provider it wraps.
struct Clocked {
    provider: Arc<ScriptedProvider>,
    calls: Arc<Mutex<Vec<tokio::time::Instant>>>,
}

#[async_trait]
impl Provider for Clocked {
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        self.calls.lock().unwrap().push(tokio::time::Instant::now());
        self.provider.complete(settings, request).await
    }
}

fn unavailable(retryable: bool) -> Failure {
    Failure {
        code: "provider.payment_required".into(),
        message: "account returned 402".into(),
        retryable,
    }
}

/// Reply candidates: the primary model under two names, which share one combination, then a
/// backup model.
fn candidates() -> Value {
    let mut document = configuration();
    let endpoints = document["endpoint"].as_object_mut().unwrap();
    let mut primary = endpoints.remove("reply").unwrap();
    primary["model"] = "primary".into();
    let mut backup = primary.clone();
    backup["model"] = "backup".into();
    endpoints.insert("primary-chat".into(), primary.clone());
    endpoints.insert("primary-alias".into(), primary);
    endpoints.insert("backup-chat".into(), backup);
    document["profile"]["default"]["reply"] =
        json!(["primary-chat", "primary-alias", "backup-chat"]);
    document
}

/// A reply window that a read or two of the history outgrows.
fn narrow(endpoint: &mut Value, window_tokens: u32) {
    endpoint["window_tokens"] = window_tokens.into();
    endpoint["max_output_tokens"] = 256.into();
}

fn write_history(root: &Path) -> String {
    let text = "some detailed history ".repeat(450);
    std::fs::write(root.join("workspace/history.txt"), &text).unwrap();
    text
}

/// Replies that each read the long history once.
fn reads(count: usize) -> impl Iterator<Item = Result<Completion, Failure>> {
    (0..count).map(|_| calls(vec![call("fs_read", r#"{"path":"history.txt"}"#)]))
}

async fn send(kernel: &Kernel, text: &str) -> Vec<Entry> {
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(session.id, EventId::new(), text.into(), Vec::new())
        .await
        .unwrap();
    finish(&mut rx).await
}

fn modes(entries: &[Entry]) -> Vec<bool> {
    entries
        .iter()
        .filter_map(|entry| match entry.body {
            EntryBody::RoundStarted { safe_mode, .. } => Some(safe_mode),
            _ => None,
        })
        .collect()
}

fn attempts(entries: &[Entry]) -> Vec<(GenerationId, &str, ContentHash)> {
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::AttemptStarted {
                provider,
                settings,
                plan,
                ..
            } => Some((*provider, settings.model.as_str(), *plan)),
            _ => None,
        })
        .collect()
}

fn models(entries: &[Entry]) -> Vec<&str> {
    attempts(entries)
        .into_iter()
        .map(|(_, model, _)| model)
        .collect()
}

/// The purposes of each Round's Attempts, in order.
fn rounds(entries: &[Entry]) -> Vec<Vec<AttemptPurpose>> {
    let mut rounds = vec![];
    for entry in entries {
        match &entry.body {
            EntryBody::RoundStarted { .. } => rounds.push(vec![]),
            EntryBody::AttemptStarted { purpose, .. } => {
                rounds.last_mut().unwrap().push(*purpose);
            }
            _ => {}
        }
    }
    rounds
}

fn run_failure(entries: &[Entry]) -> Option<&Failure> {
    match &entries.last()?.body {
        EntryBody::RunEnded {
            end: RunEnd::Failed { failure },
            ..
        } => Some(failure),
        _ => None,
    }
}

#[tokio::test]
async fn exhausted_combinations_survive_unrelated_registration_and_reset_with_the_run() {
    for empty in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let primary = ScriptedProvider::new(vec![
            if empty {
                reply("")
            } else {
                Err(unavailable(false))
            },
            reply("primary recovered"),
        ]);
        let backup = ScriptedProvider::new(vec![]);
        let provider = Arc::new(Models(BTreeMap::from([
            ("primary", primary.clone() as Arc<dyn Provider>),
            ("backup", backup.clone() as Arc<dyn Provider>),
        ])));
        let (kernel, _) =
            kernel_configured(dir.path(), provider, candidates(), vec![], |_| {}).await;
        let mut changed = candidates();
        changed["run"] = json!({"max_rounds": 30});
        std::fs::write(
            dir.path().join("config.toml"),
            toml::to_string(&changed).unwrap(),
        )
        .unwrap();
        backup.steps.lock().unwrap().extend([
            calls(vec![call(
                "self_apply",
                r#"{"reason":"Apply test intent"}"#,
            )]),
            reply("backup finished"),
        ]);
        let log = send(&kernel, "work").await;
        assert_eq!(modes(&log), [false, false]);
        assert_eq!(models(&log), ["primary", "backup", "backup"]);
        assert_eq!(
            kernel.registry().view().config.document["run"]["max_rounds"],
            30
        );
        assert_eq!(primary.requests.lock().unwrap().len(), 1); // The alias has the same combination.
        let prompt = backup.requests.lock().unwrap()[0].messages[0].joined_text();
        let failure = if empty {
            "no text or tool calls"
        } else {
            "402"
        };
        assert!(
            prompt.contains("primary-chat")
                && prompt.contains("primary-alias")
                && prompt.contains(failure)
        );
        if empty {
            assert!(
                log.iter()
                    .any(|entry| matches!(&entry.body, EntryBody::AttemptSettled {
                result: AttemptResult::Completed { message, usage, .. }, ..
            } if message.joined_text().is_empty() && usage.output_tokens == 1))
            );
            // Recorded as returned, the empty reply is still not part of the conversation.
            let unusable = |message: &Message<ImageData>| {
                message.role == Role::Assistant
                    && message.joined_text().is_empty()
                    && message.tool_calls().next().is_none()
            };
            assert!(
                !backup.requests.lock().unwrap()[1]
                    .messages
                    .iter()
                    .any(unusable)
            );
        }
        let next = send(&kernel, "again").await;
        assert_eq!(models(&next), ["primary"]);
        kernel.shutdown().await.unwrap();
    }
}

async fn registry_node(
    root: &Path,
    factory: Arc<dyn Provider>,
    trial: Arc<dyn Provider>,
) -> (Kernel, GenerationId, GenerationId) {
    let runtime = factory_runtime(fixture(factory, ScriptedProvider::new(vec![])));
    runtime.insert(b"trial", fixture(trial, ScriptedProvider::new(vec![])));
    let (development, registry, store) =
        scripted_home(root, runtime, configuration(), vec![]).await;
    let original = registry
        .view()
        .exports
        .completion(&fixture_endpoint("fixture"), true)
        .unwrap()
        .generation;
    let trial = source_fixture(root, &store, &development, &registry, "fixture", b"trial")
        .await
        .generations[0]
        .id;
    std::fs::remove_dir_all(root.join("plugins/fixture")).unwrap();
    let kernel = Kernel::start(kernel_deps(root, store, registry, None, Some(development)))
        .await
        .unwrap();
    (kernel, original, trial)
}

#[tokio::test]
async fn automatic_safe_mode_tracks_repair_and_starts_each_run_afresh() {
    for repair in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let factory = ScriptedProvider::new(vec![]);
        let trial = ScriptedProvider::new(vec![Err(unavailable(false)), Err(unavailable(false))]);
        let (kernel, original, broken) = registry_node(dir.path(), factory.clone(), trial).await;
        let repair_call = if repair {
            call("self_apply", r#"{"reason":"Apply test intent"}"#)
        } else {
            call("fs_list", r#"{"path":"."}"#)
        };
        factory.steps.lock().unwrap().extend([
            calls(vec![repair_call]),
            reply("finished"),
            reply("next run"),
        ]);
        let log = send(&kernel, "repair yourself").await;
        assert_eq!(
            modes(&log),
            if repair {
                vec![false, true, false]
            } else {
                vec![false, true, true]
            }
        );
        let failed = log
            .iter()
            .find_map(|entry| match &entry.body {
                EntryBody::RoundEnded {
                    end: RoundEnd::Failed { generations, .. },
                    ..
                } => Some(generations),
                _ => None,
            })
            .unwrap();
        assert!(failed.contains(&broken) && !failed.contains(&original));
        assert!(failed.windows(2).all(|ids| ids[0] < ids[1]));
        assert!(
            factory.requests.lock().unwrap()[0].messages[0]
                .joined_text()
                .contains("402")
        );
        let next = send(&kernel, "new work").await;
        assert_eq!(
            modes(&next),
            if repair {
                vec![false]
            } else {
                vec![false, true]
            }
        );
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn a_third_attempt_rollback_grants_the_new_generation_its_own_allowance() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let clocked = |steps: Vec<Result<Completion, Failure>>| {
        Arc::new(Clocked {
            provider: ScriptedProvider::new(steps),
            calls: calls.clone(),
        })
    };
    let factory = clocked(vec![
        Err(unavailable(true)),
        Err(unavailable(true)),
        reply("restored"),
        reply("rollback notice received"),
    ]);
    let trial = clocked(vec![
        Err(unavailable(true)),
        Err(unavailable(true)),
        Err(Failure {
            code: code::PLUGIN_TRAP.into(),
            message: "broken code".into(),
            retryable: false,
        }),
    ]);
    let (kernel, original, broken) = registry_node(dir.path(), factory, trial).await;
    let log = send(&kernel, "hello").await;
    let attempts = attempts(&log);
    assert_eq!(
        attempts[..6]
            .iter()
            .map(|(generation, _, _)| *generation)
            .collect::<Vec<_>>(),
        [broken, broken, broken, original, original, original]
    );
    assert!(
        attempts[..6]
            .iter()
            .all(|(_, _, plan)| *plan == attempts[0].2)
    );
    {
        // Retries back off; the rolled-back generation is called at the instant of the trap,
        // and its own retries back off from the start.
        let calls = calls.lock().unwrap();
        let gaps: Vec<_> = calls.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(gaps[0] > Duration::ZERO);
        assert_eq!(gaps[2], Duration::ZERO);
        assert_eq!(gaps[3..5], gaps[..2]);
    }
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_round_records_the_generations_after_rollback_and_does_not_retry_factory() {
    let dir = tempfile::tempdir().unwrap();
    let factory = ScriptedProvider::new(vec![Err(unavailable(false))]);
    let trial = ScriptedProvider::new(vec![Err(Failure {
        code: code::PLUGIN_TRAP.into(),
        message: "trap".into(),
        retryable: false,
    })]);
    let (kernel, original, broken) = registry_node(dir.path(), factory.clone(), trial).await;
    let log = send(&kernel, "hello").await;
    assert_eq!(modes(&log), [false, true]);
    assert_eq!(
        attempts(&log)
            .iter()
            .map(|(generation, _, _)| *generation)
            .collect::<Vec<_>>(),
        [broken, original]
    );
    for generations in log.iter().filter_map(|entry| match &entry.body {
        EntryBody::RoundEnded {
            end: RoundEnd::Failed { generations, .. },
            ..
        } => Some(generations),
        _ => None,
    }) {
        assert!(generations.contains(&original) && !generations.contains(&broken));
    }
    assert_eq!(factory.requests.lock().unwrap().len(), 1);
    kernel.shutdown().await.unwrap();
}

struct PurposeProvider {
    requests: Mutex<Vec<ProviderRequest>>,
}
#[async_trait]
impl Provider for PurposeProvider {
    async fn complete(
        &self,
        _: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        let summary = request.tools.is_empty();
        self.requests.lock().unwrap().push(request);
        if summary {
            reply("a concise summary")
        } else {
            Err(unavailable(false))
        }
    }
}

#[tokio::test]
async fn reply_exhaustion_leaves_the_same_service_available_for_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let primary = Arc::new(PurposeProvider {
        requests: Mutex::new(vec![]),
    });
    let backup = ScriptedProvider::new(reads(6).chain([reply("done")]).collect());
    let provider = Arc::new(Models(BTreeMap::from([
        ("primary", primary.clone() as Arc<dyn Provider>),
        ("backup", backup.clone() as Arc<dyn Provider>),
    ])));
    let mut document = candidates();
    for endpoint in ["primary-chat", "primary-alias", "backup-chat"] {
        narrow(&mut document["endpoint"][endpoint], 6000);
    }
    document["endpoint"]["compaction"]["model"] = "primary".into();
    let (kernel, _) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
    write_history(dir.path());
    let log = send(&kernel, "read and summarize").await;
    assert!(matches!(
        log.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    assert!(
        log.iter()
            .any(|entry| matches!(entry.body, EntryBody::Compacted { .. }))
    );
    {
        let requests = primary.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| !request.tools.is_empty())
                .count(),
            1
        );
        assert!(requests.iter().any(|request| request.tools.is_empty()));
        assert!(
            requests
                .iter()
                .filter(|request| request.tools.is_empty())
                .all(|request| !request.messages[0].joined_text().contains("402"))
        );
    }
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_round_ends_the_run_when_no_safe_mode_round_may_follow() {
    // The owner's switch is on, or the Round budget is spent.
    for switch in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let provider = ScriptedProvider::new(vec![Err(unavailable(false))]);
        let mut document = configuration();
        if !switch {
            document["run"] = json!({"max_rounds": 1});
        }
        let (kernel, _) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
        kernel.set_safe_mode(switch).await.unwrap();
        let log = send(&kernel, "hello").await;
        assert_eq!(modes(&log), [switch]);
        assert_eq!(run_failure(&log), Some(&unavailable(false)));
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn applying_calls_every_candidate_once_and_a_failing_backup_rejects_it() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![reply("probe"), Err(unavailable(false))]);
    let (registry, _) = registry_configured(
        dir.path(),
        fixture(provider.clone(), provider.clone()),
        configuration(),
        vec![],
    )
    .await;
    let before = registry.view().config.generation;
    let result = submit(&registry, configuration_request(candidates())).await;
    assert!(matches!(result, Err(RegistryError::Rejected(causes))
        if matches!(causes.as_slice(), [Cause::Use { endpoint, failure, .. }]
            if endpoint == "backup-chat" && *failure == unavailable(false))));
    assert_eq!(registry.view().config.generation, before);
    // The alias shares the primary's call parameters, so the two make one call.
    assert_eq!(
        provider
            .settings
            .lock()
            .unwrap()
            .iter()
            .map(|settings| settings.model.as_str())
            .collect::<Vec<_>>(),
        ["primary", "backup"]
    );
}

#[tokio::test]
async fn a_profile_without_candidates_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let (registry, _) = registry_configured(
        dir.path(),
        fixture(provider.clone(), provider),
        configuration(),
        vec![],
    )
    .await;
    for field in ["reply", "compaction"] {
        let mut empty = configuration();
        empty["profile"]["default"][field] = json!([]);
        let result = submit(&registry, configuration_request(empty)).await;
        assert!(matches!(result, Err(RegistryError::Rejected(causes))
            if matches!(causes.as_slice(), [Cause::Table(error)] if error.table == "profile.default")));
    }
}

struct FailingSecondContext(std::sync::atomic::AtomicUsize);

#[async_trait]
impl ContextSource for FailingSecondContext {
    async fn contribute(&self, _: &ContextQuery) -> Result<Contribution, ContextError> {
        if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            Err(ContextError("authority unavailable".into()))
        } else {
            Ok(Contribution::default())
        }
    }
}

#[tokio::test]
async fn safe_mode_keeps_factory_combinations_exhausted_and_context_is_sampled_once() {
    let dir = tempfile::tempdir().unwrap();
    let primary = ScriptedProvider::new(vec![Err(unavailable(false))]);
    let backup = ScriptedProvider::new(vec![
        calls(vec![call("fs_list", r#"{"path":"."}"#)]),
        reply("recovered"),
    ]);
    let provider = Arc::new(Models(BTreeMap::from([
        ("primary", primary.clone() as Arc<dyn Provider>),
        ("backup", backup as Arc<dyn Provider>),
    ])));
    let context = Arc::new(FailingSecondContext(std::sync::atomic::AtomicUsize::new(0)));
    let (kernel, _) = kernel_configured(dir.path(), provider, candidates(), vec![], |deps| {
        deps.context.push(context.clone())
    })
    .await;
    let log = send(&kernel, "continue despite failures").await;
    assert_eq!(modes(&log), [false, false, true]);
    assert_eq!(models(&log), ["primary", "backup", "backup"]);
    assert_eq!(context.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(primary.requests.lock().unwrap().len(), 1);
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_empty_or_cut_off_summary_falls_back_to_the_next_compaction_budget() {
    let cut_off = Completion {
        stop: StopReason::MaxTokens,
        ..reply("The owner asked to read the history and").unwrap()
    };
    for unusable in [reply(""), Ok(cut_off)] {
        let dir = tempfile::tempdir().unwrap();
        let replies = ScriptedProvider::new(reads(6).chain([reply("done")]).collect());
        let primary = ScriptedProvider::new(vec![unusable]);
        let backup = ScriptedProvider::new((0..16).map(|_| reply("a concise summary")).collect());
        let provider = Arc::new(Models(BTreeMap::from([
            ("fixture", replies as Arc<dyn Provider>),
            ("summary", primary.clone() as Arc<dyn Provider>),
            ("backup-summary", backup.clone() as Arc<dyn Provider>),
        ])));
        let mut document = configuration();
        narrow(&mut document["endpoint"]["reply"], 6000);
        document["endpoint"]["compaction"]["model"] = "summary".into();
        document["endpoint"]["backup"] = document["endpoint"]["compaction"].clone();
        document["endpoint"]["backup"]["model"] = "backup-summary".into();
        document["endpoint"]["backup"]["window_tokens"] = 4000.into();
        document["endpoint"]["backup"]["max_output_tokens"] = 256.into();
        document["profile"]["default"]["compaction"] = json!(["compaction", "backup"]);
        let (kernel, _) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
        write_history(dir.path());
        let log = send(&kernel, "read and summarize").await;
        assert!(matches!(
            log.last().unwrap().body,
            EntryBody::RunEnded {
                end: RunEnd::Completed,
                ..
            }
        ));
        // Each compaction model gets the output budget its endpoint declares.
        {
            let requests = primary.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].max_output_tokens, Some(8192));
        }
        {
            let requests = backup.requests.lock().unwrap();
            assert!(!requests.is_empty());
            assert!(
                requests
                    .iter()
                    .all(|request| request.max_output_tokens == Some(256)
                        && request.tools.is_empty())
            );
        }
        assert!(log.iter().all(|entry| !matches!(
            &entry.body,
            EntryBody::Compacted { summary, .. } if summary != "a concise summary"
        )));
        kernel.shutdown().await.unwrap();
    }
}

struct ObservedComposer {
    inner: Arc<dyn Composer>,
    inputs: Arc<Mutex<Vec<ComposeInput>>>,
}
impl Composer for ObservedComposer {
    fn code(&self) -> CodeRef {
        self.inner.code()
    }
    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError> {
        self.inputs.lock().unwrap().push(input.clone());
        self.inner.compose(input)
    }
}

#[expect(
    clippy::panic,
    reason = "calibration fails the test if the composer does not report its needed size"
)]
async fn measured_reply_window(failure: &Failure) -> u32 {
    let root = tempfile::tempdir().unwrap();
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(reads(2).chain([reply("read")]).collect());
    let mut document = configuration();
    document["endpoint"]["reply"]["max_output_tokens"] = 256.into();
    let mut composer = None;
    let (kernel, _) = kernel_configured(root.path(), provider, document, vec![], |deps| {
        composer = Some(deps.composer.clone());
        deps.composer = Arc::new(ObservedComposer {
            inner: deps.composer.clone(),
            inputs: inputs.clone(),
        });
    })
    .await;
    let composer = composer.unwrap();
    let history = write_history(root.path());
    send(&kernel, "read the history twice").await;
    kernel.shutdown().await.unwrap();
    let mut costs = BTreeMap::new();
    for mut input in inputs.lock().unwrap().drain(..) {
        let reads = input.transcript.items.iter().flat_map(|item| &item.message.parts)
            .filter(|part| matches!(part, Part::ToolResult(result) if result.content.ends_with(&history))).count();
        // Ask the real composer for its needed size, with compaction disabled for this probe.
        input.reply.budget.context_tokens = std::num::NonZeroU32::MIN;
        input.compactions_left = 0;
        for safe in [false, true] {
            if safe {
                input.safe_mode = Some(SafeMode::Failed(failure.clone()));
                input
                    .tools
                    .retain(|(_, tool)| enco_host::LIFELINE.contains(&tool.name.as_str()));
            }
            let Err(ComposeError::ContextOverflow { needed, .. }) = composer.compose(&input) else {
                panic!("calibration must report the complete request size");
            };
            costs.insert((reads, safe), needed);
        }
    }
    // One read must fit normally; two must overflow even with the smaller lifeline catalog.
    let (one, two) = (
        costs[&(1, false)],
        costs[&(2, false)].min(costs[&(2, true)]),
    );
    assert!(
        one < two,
        "history must outgrow both tool catalogs: {costs:?}"
    );
    let window = one + (two - one) / 2;
    assert!(one < window && window < two);
    window
}

#[tokio::test]
async fn without_a_compaction_model_only_a_reply_that_fits_proceeds() {
    let dir = tempfile::tempdir().unwrap();
    let summary_down = Failure {
        code: "provider.overloaded".into(),
        message: "summary model overloaded".into(),
        retryable: false,
    };
    let provider = Arc::new(Models(BTreeMap::from([
        (
            "fixture",
            ScriptedProvider::new(reads(2).collect()) as Arc<dyn Provider>,
        ),
        (
            "summary",
            ScriptedProvider::new(vec![Err(summary_down.clone())]) as Arc<dyn Provider>,
        ),
    ])));
    let mut document = configuration();
    narrow(
        &mut document["endpoint"]["reply"],
        measured_reply_window(&summary_down).await,
    );
    document["endpoint"]["compaction"]["model"] = "summary".into();
    let (kernel, _) = kernel_configured(dir.path(), provider, document, vec![], |_| {}).await;
    write_history(dir.path());
    let log = send(&kernel, "read the history twice").await;
    // Neither the third Round nor its safe-mode successor makes a request.
    assert_eq!(
        rounds(&log),
        [
            vec![AttemptPurpose::Reply],
            vec![AttemptPurpose::Compaction, AttemptPurpose::Reply],
            vec![],
            vec![],
        ]
    );
    for entry in &log {
        if let EntryBody::RoundEnded {
            end: RoundEnd::Failed { failure, .. },
            ..
        } = &entry.body
        {
            assert_eq!(failure, &summary_down);
        }
    }
    assert_eq!(run_failure(&log), Some(&summary_down));
    kernel.shutdown().await.unwrap();
}
