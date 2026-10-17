#![expect(clippy::unwrap_used, reason = "behavior fixtures fail with context")]
mod support;

use enco_core::*;
use enco_host::{
    SqliteStore,
    development::*,
    status::{AttentionKind, CallStep, Divergence, FixTarget, Status, Step, Steps},
};
use enco_kernel::*;
use serde_json::{Map, Value, json};
use std::{path::Path, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;

struct Node {
    development: Arc<Development>,
    registry: Arc<Registry>,
    store: Arc<SqliteStore>,
    kernel: Kernel,
}

impl Node {
    async fn start(root: &Path) -> Self {
        let provider = ScriptedProvider::new(vec![]);
        let runtime = factory_runtime(fixture(provider.clone(), provider.clone()));
        for bytes in [b"one".as_slice(), b"two"] {
            runtime.insert(bytes, fixture(provider.clone(), provider.clone()));
        }
        let (development, registry, store) =
            scripted_home(root, runtime, configuration(), vec![]).await;
        let kernel = Kernel::start(kernel_deps(
            root,
            store.clone(),
            registry.clone(),
            None,
            Some(development.clone()),
        ))
        .await
        .unwrap();
        Self {
            development,
            registry,
            store,
            kernel,
        }
    }

    async fn source(&self, root: &Path, name: &str, bytes: &[u8]) -> Registered {
        source_fixture(
            root,
            &self.store,
            &self.development,
            &self.registry,
            name,
            bytes,
        )
        .await
    }

    async fn status(&self, root: &Path) -> Status {
        current_status(root, &self.development, &self.kernel).await
    }

    async fn apply(&self) -> Applied {
        self.development
            .apply(
                &self.registry,
                Initiator::Owner { reason: None },
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
}

fn attention(status: &Status, kind: &str) -> Value {
    serde_json::to_value(status).unwrap()["needs_attention"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == kind)
        .unwrap()
        .clone()
}

fn steps(status: &Status, kind: impl Fn(&AttentionKind) -> bool) -> Steps {
    status
        .attention()
        .into_iter()
        .find(|item| kind(&item.kind))
        .unwrap()
        .next
}

#[tokio::test]
async fn workspace_attention_distinguishes_meaning_build_evidence_and_read_failures() {
    let root = tempfile::tempdir().unwrap();
    let node = Node::start(root.path()).await;
    node.source(root.path(), "alpha", b"one").await;
    let path = root.path().join("plugins/alpha/source.txt");
    std::fs::write(&path, b"one with a comment").unwrap();
    attention(&node.status(root.path()).await, "unbuilt");
    retain_build(root.path(), &node.store, path.parent().unwrap(), b"one").await;
    let config = root.path().join("config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str("\n# Owner comment\n");
    std::fs::write(&config, &text).unwrap();
    let applied = node.apply().await;
    assert!(applied.registered.is_none());
    assert_eq!(
        applied.text_only,
        [IntentPath::Configuration, "plugins/alpha".parse().unwrap()]
    );
    let status = node.status(root.path()).await;
    assert!(status.attention().is_empty());
    assert_eq!(
        status.development.configuration.workspace,
        WorkspaceComparison::TextOnly
    );
    assert_eq!(
        status.development.plugins["alpha"]
            .workspace
            .as_ref()
            .unwrap()
            .code,
        WorkspaceComparison::TextOnly
    );
    text.push_str("\n[plugin.alpha.config]\nlabel = 'edited'\n");
    std::fs::write(&config, &text).unwrap();
    let status = node.status(root.path()).await;
    assert_eq!(status.attention().len(), 1);
    assert_eq!(attention(&status, "unregistered")["path"], "config.toml");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    assert_eq!(
        attention(&node.status(root.path()).await, "removed")["path"],
        "plugins/alpha"
    );
    std::fs::remove_dir(root.path().join("plugins")).unwrap();
    std::fs::write(root.path().join("plugins"), "not a directory").unwrap();
    let status = node.status(root.path()).await;
    assert_eq!(
        status.development.plugins_unreadable.as_ref().unwrap().path,
        root.path().join("plugins")
    );
    attention(&status, "plugins_unreadable");
    node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn runtime_attention_uses_the_selected_generation_and_keeps_running_source_only_when_it_differs()
 {
    let root = tempfile::tempdir().unwrap();
    let node = Node::start(root.path()).await;
    let first = node.source(root.path(), "alpha", b"one").await;
    for _ in 0..TRIAL_CALLS {
        node.registry
            .report(first.generations[0].id, Verdict::Ok, None)
            .await
            .unwrap();
    }
    let second = node.source(root.path(), "alpha", b"two").await;
    node.registry.rollback().await.unwrap();
    let status = node.status(root.path()).await;
    let diverged = attention(&status, "diverged");
    assert_eq!(
        diverged["divergence"]["registered"]["generation"],
        json!(second.generations[0].id)
    );
    assert_eq!(
        diverged["next"]["first"][0]["arguments"],
        json!({"registration":first.registration.id,"paths":["plugins/alpha"]})
    );
    // A new deployment can fail while the running source still has the original provenance.
    let broken = node
        .development
        .deploy(
            &node.registry,
            "alpha".parse().unwrap(),
            b"two".to_vec(),
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let failure = Failure {
        code: code::PLUGIN_TRAP.into(),
        message: "fixture trap".into(),
        retryable: false,
    };
    node.registry
        .report(
            broken.generations[0].id,
            Verdict::Failed(failure.clone()),
            None,
        )
        .await
        .unwrap();
    let rolled = attention(&node.status(root.path()).await, "rolled_back");
    assert_eq!(rolled["cause"]["failure"], json!(failure));
    assert_eq!(
        rolled["next"]["alternative"][0]["arguments"]["registration"],
        json!(first.registration.id)
    );
    let alone = node.source(root.path(), "alone", b"one").await;
    node.registry
        .report(alone.generations[0].id, Verdict::Failed(failure), None)
        .await
        .unwrap();
    assert_eq!(
        attention(&node.status(root.path()).await, "stopped")["path"],
        "plugins/alone"
    );
    let mut document = configuration();
    document["run"] = json!({"max_rounds": 23});
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    node.apply().await;
    node.registry.rollback().await.unwrap();
    attention(&node.status(root.path()).await, "configuration_diverged");
    node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn owner_switch_and_stopped_owners_are_reported_from_execution_facts() {
    let root = tempfile::tempdir().unwrap();
    let runtime = factory_runtime(fixture(
        ScriptedProvider::new(vec![]),
        ScriptedProvider::new(vec![]),
    ));
    let (development, registry, store) =
        scripted_home(root.path(), runtime, configuration(), vec![]).await;
    let session = store.ensure_session("broken", Utc::now()).await.unwrap();
    // A future Log body is valid JSON but unreadable by this host's Session owner.
    rusqlite::Connection::open(root.path().join("enco.db"))
        .unwrap()
        .execute(
            "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,1,1,?,?)",
            rusqlite::params![
                session.id.to_string(),
                session.created_at.to_rfc3339(),
                r#"{"kind":"future_record"}"#
            ],
        )
        .unwrap();
    store
        .insert_schedule(&Schedule {
            id: ScheduleId::new(),
            session: session.id,
            rule: ScheduleRule::Cron {
                expr: "invalid".into(),
                timezone: None,
            },
            message: "briefing".into(),
            created_at: session.created_at,
            state: ScheduleState::Active,
            last: None,
        })
        .await
        .unwrap();
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry,
        None,
        Some(development.clone()),
    ))
    .await
    .unwrap();
    kernel.set_safe_mode(true).await.unwrap();
    assert!(kernel.schedules().list(None).await.is_err());
    kernel.shutdown().await.unwrap();
    let status = current_status(root.path(), &development, &kernel).await;
    for kind in ["safe_mode", "scheduler_stopped", "session_stopped"] {
        attention(&status, kind);
    }
}

/// The match has no wildcard, so a new step kind fails to compile until it is chained in here.
fn successor(step: &CallStep) -> Option<CallStep> {
    let registration = RegistrationId(3);
    let rewind = RewindArguments {
        registration,
        paths: Some(vec!["plugins/web".parse().unwrap()]),
    };
    let (name, to): (PluginName, PluginName) = ("old".parse().unwrap(), "new".parse().unwrap());
    Some(match step {
        CallStep::Review(_) => CallStep::Compare(HistoryQuery::Compare {
            from: Some(HistoryPoint::Workspace),
            to: HistoryPoint::Registration(registration),
        }),
        CallStep::Compare(_) => CallStep::List(Some(registration)),
        CallStep::List(_) => CallStep::Show(registration),
        CallStep::Show(_) => CallStep::Register,
        CallStep::Register => CallStep::RegisterRemoval,
        CallStep::RegisterRemoval => CallStep::Discard {
            latest: registration,
            paths: vec![IntentPath::Configuration],
        },
        CallStep::Discard { .. } => CallStep::KeepRunning(rewind),
        CallStep::KeepRunning(_) => CallStep::Restore(rewind),
        CallStep::Restore(_) => CallStep::Rewind(RewindArguments {
            registration,
            paths: None,
        }),
        CallStep::Rewind(_) => CallStep::Overwrite(rewind),
        CallStep::Overwrite(_) => CallStep::RecordName { name, to },
        CallStep::RecordName { .. } => CallStep::Rename { name, to },
        CallStep::Rename { .. } => CallStep::Recheck,
        CallStep::Recheck => return None,
    })
}

fn every_call_step() -> Vec<CallStep> {
    let first = CallStep::Review(HistoryQuery::Compare {
        from: None,
        to: HistoryPoint::Workspace,
    });
    std::iter::successors(Some(first), successor).collect()
}

/// Parse as execution does: the tool's name selects the one parser of that tool.
fn parse_as_executed(tool: &str, arguments: Map<String, Value>) -> Result<Value, Failure> {
    match DevelopmentTool::named(tool) {
        Some(tool) => tool
            .parse(arguments)
            .map(|call| serde_json::to_value(call).unwrap()),
        None => {
            assert_eq!(tool, STATUS);
            StatusArguments::parse(arguments).map(|parsed| serde_json::to_value(parsed).unwrap())
        }
    }
}

#[test]
fn every_suggested_call_parses_with_the_parser_execution_uses() {
    let reason = json!("Keep the running behavior");
    for step in every_call_step() {
        let invocation = step.invocation();
        let Value::Object(mut arguments) = serde_json::to_value(&invocation.arguments).unwrap()
        else {
            panic!("{step:?} suggests arguments that are not an object");
        };
        for field in invocation.agent_supplies {
            assert_eq!(arguments[field], REASON_PLACEHOLDER, "{step:?}");
            assert_eq!(
                parse_as_executed(invocation.tool, arguments.clone())
                    .unwrap_err()
                    .code,
                code::TOOL_INVALID_ARGUMENTS,
                "{step:?} must not run with the placeholder"
            );
            arguments.insert(field.into(), reason.clone());
        }
        assert_eq!(
            parse_as_executed(invocation.tool, arguments.clone()).unwrap(),
            Value::Object(arguments),
            "{step:?}"
        );
    }
}

#[test]
fn a_rename_that_fails_after_moving_moves_the_directory_back_before_renaming_again() {
    let (name, to): (PluginName, PluginName) = ("old".parse().unwrap(), "new".parse().unwrap());
    for error in [
        RegistryError::Conflict("registration baseline".into()),
        RegistryError::Rejected(vec![Cause::NameTaken { name: to.clone() }]),
    ] {
        let failed = Failed {
            operation: Operation::Rename {
                name: name.clone(),
                to: to.clone(),
            },
            error: Box::new(DevelopmentError::Moved(Box::new(
                DevelopmentError::Registry(error),
            ))),
        };
        assert_eq!(failed.code(), code::PLUGIN_REJECTED);
        let next = failed.next();
        assert_eq!(
            next.first,
            [
                Step::Fix(FixTarget::MoveDirectory {
                    from: IntentPath::Plugin(to.clone()),
                    to: IntentPath::Plugin(name.clone()),
                }),
                Step::Call(CallStep::Rename {
                    name: name.clone(),
                    to: to.clone(),
                }),
            ]
        );
        assert!(next.alternative.is_none());
    }
}

#[tokio::test]
async fn fallback_to_factory_exposes_an_unserved_use_without_inventing_source_to_rewind() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![]);
    let runtime = factory_runtime(fixture(provider.clone(), provider.clone()));
    let mut optional = fixture(provider.clone(), provider);
    optional.credentials = vec![credential(false)];
    runtime.insert(b"optional", optional);
    let (development, registry, store) =
        scripted_home(root.path(), runtime, configuration(), vec![]).await;
    let trial = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"optional",
    )
    .await;
    let mut document = configuration();
    document["endpoint"]["embed"]["api_key_env"] = "FIXTURE_KEY".into();
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    development
        .apply(
            &registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    registry
        .report(
            trial.generations[0].id,
            Verdict::Failed(Failure {
                code: code::PLUGIN_TRAP.into(),
                message: "fixture trap".into(),
                retryable: false,
            }),
            None,
        )
        .await
        .unwrap();
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry,
        None,
        Some(development.clone()),
    ))
    .await
    .unwrap();
    let status = current_status(root.path(), &development, &kernel).await;
    assert_eq!(
        attention(&status, "unserved_use")["usage"]["user"],
        "memory.embedding"
    );
    assert!(
        attention(&status, "rolled_back")["next"]
            .get("alternative")
            .is_none()
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_table_the_current_host_cannot_read_is_reported_at_its_owner() {
    let root = tempfile::tempdir().unwrap();
    let runtime = factory_runtime(fixture(
        ScriptedProvider::new(vec![]),
        ScriptedProvider::new(vec![]),
    ));
    let store = Arc::new(SqliteStore::open(store_paths(root.path())).await.unwrap());
    let mut old = registry_deps(store.clone(), runtime.clone(), FACTORY_BYTES);
    old.owners
        .iter_mut()
        .find(|owner| owner.tables.iter().any(|table| table.name() == "memory"))
        .unwrap()
        .declare = |_, _| Ok(vec![]);
    let mut document = configuration();
    document["memory"]["embedding"] = "missing".into();
    let (development, registry) =
        open_home(root.path(), store.clone(), old, document.clone()).await;
    drop((development, registry));
    let (development, registry) = open_home(
        root.path(),
        store.clone(),
        registry_deps(store.clone(), runtime, FACTORY_BYTES),
        document,
    )
    .await;
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store,
        registry,
        None,
        Some(development.clone()),
    ))
    .await
    .unwrap();
    let status = current_status(root.path(), &development, &kernel).await;
    assert_eq!(
        attention(&status, "invalid_table")["error"]["table"],
        "memory"
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn registered_code_provenance_follows_the_selected_generation_through_settings() {
    let root = tempfile::tempdir().unwrap();
    let node = Node::start(root.path()).await;
    let provenance = |status: Status| {
        status.development.plugins["fixture"]
            .registered
            .as_ref()
            .unwrap()
            .plugin
            .activation
            .code
    };
    assert!(provenance(node.status(root.path()).await).source == CodeSource::Factory);
    let deployed = CodeProvenance {
        generation: node
            .development
            .deploy(
                &node.registry,
                "fixture".parse().unwrap(),
                b"one".to_vec(),
                Initiator::Owner { reason: None },
                &CancellationToken::new(),
            )
            .await
            .unwrap()
            .generations[0]
            .id,
        source: CodeSource::Artifact,
    };
    assert!(provenance(node.status(root.path()).await) == deployed);
    let mut document = configuration();
    document["plugin"]["fixture"]["config"] = json!({"label": "new settings"});
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    // A settings-only generation keeps the provenance of the code it runs.
    let settings = node.apply().await.registered.unwrap().generations[0].id;
    let status = node.status(root.path()).await;
    let registered = &status.development.plugins["fixture"]
        .registered
        .as_ref()
        .unwrap()
        .plugin
        .activation;
    assert_eq!(registered.generation.id, settings);
    assert!(registered.code == deployed);
    node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn running_source_already_registered_is_not_offered_as_a_repair() {
    let root = tempfile::tempdir().unwrap();
    let node = Node::start(root.path()).await;
    let source = node.source(root.path(), "alpha", b"one").await;
    for _ in 0..TRIAL_CALLS {
        node.registry
            .report(source.generations[0].id, Verdict::Ok, None)
            .await
            .unwrap();
    }
    let deployed = node
        .development
        .deploy(
            &node.registry,
            "alpha".parse().unwrap(),
            b"two".to_vec(),
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    node.registry
        .report(
            deployed.generations[0].id,
            Verdict::Failed(Failure {
                code: code::PLUGIN_TRAP.into(),
                message: "component trap".into(),
                retryable: false,
            }),
            None,
        )
        .await
        .unwrap();
    let status = node.status(root.path()).await;
    assert!(
        attention(&status, "rolled_back")["next"]
            .get("alternative")
            .is_none()
    );
    assert!(node.apply().await.registered.is_none());
    node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn new_plugins_and_each_runtime_difference_offer_scoped_repairs() {
    let root = tempfile::tempdir().unwrap();
    let node = Node::start(root.path()).await;
    std::fs::create_dir(root.path().join("plugins/draft")).unwrap();
    std::fs::write(root.path().join("plugins/draft/source.txt"), "draft").unwrap();
    let draft: IntentPath = "plugins/draft".parse().unwrap();
    let new = steps(
        &node.status(root.path()).await,
        |kind| matches!(kind, AttentionKind::New { path } if *path == draft),
    );
    assert_eq!(new.first, [Step::Call(CallStep::Register)]);
    std::fs::remove_dir_all(root.path().join("plugins/draft")).unwrap();
    node.source(root.path(), "alpha", b"one").await;
    node.development
        .rename(
            &node.registry,
            &"alpha".parse().unwrap(),
            &"beta".parse().unwrap(),
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    node.registry.rollback().await.unwrap();
    let name = steps(&node.status(root.path()).await, |kind| {
        matches!(kind, AttentionKind::Diverged { divergence: Divergence::Name { running, .. }, .. }
            if running.as_str() == "alpha")
    });
    assert_eq!(
        name.first,
        [Step::Call(CallStep::RecordName {
            name: "beta".parse().unwrap(),
            to: "alpha".parse().unwrap(),
        })]
    );
    assert!(name.alternative.is_none());
    let mut document = configuration();
    document["plugin"]["fixture"]["config"] = json!({"label": "new settings"});
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    node.apply().await;
    node.registry.rollback().await.unwrap();
    let settings = steps(&node.status(root.path()).await, |kind| {
        matches!(
            kind,
            AttentionKind::Diverged {
                divergence: Divergence::Settings { .. },
                ..
            }
        )
    });
    assert!(matches!(settings.first.as_slice(), [
        Step::Call(CallStep::Compare(_)),
        Step::Fix(FixTarget::PluginSettings { name }),
        Step::Call(CallStep::Register),
    ] if name.as_str() == "fixture"));
    assert!(settings.alternative.is_none());
    let removal_target = node.source(root.path(), "removed", b"one").await;
    for _ in 0..TRIAL_CALLS {
        node.registry
            .report(removal_target.generations[0].id, Verdict::Ok, None)
            .await
            .unwrap();
    }
    std::fs::remove_dir_all(root.path().join("plugins/removed")).unwrap();
    let removed = node.apply().await.registered.unwrap().registration.id;
    node.registry.rollback().await.unwrap();
    let restored = steps(&node.status(root.path()).await, |kind| {
        matches!(kind, AttentionKind::Diverged { divergence: Divergence::Unregistered { removed_by, .. }, .. }
            if *removed_by == removed)
    });
    assert!(matches!(
        restored.first.as_slice(),
        [Step::Call(CallStep::Compare(_))]
    ));
    node.kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn rename_precheck_rejection_keeps_its_plugin_error_code() {
    let root = tempfile::tempdir().unwrap();
    let node = Node::start(root.path()).await;
    let tool = enco_host::development_tools(node.development.clone(), node.registry.clone())
        .into_iter()
        .find(|tool| tool.spec().name == RENAME)
        .unwrap();
    let session = node.kernel.open_session("rename").await.unwrap();
    let outcome = tool
        .call(
            CallContext {
                session: session.id,
                call: CallId::new(),
                cancel: CancellationToken::new(),
                result_budget: 10000,
            },
            json!({"name":"fixture", "to":"renamed", "reason":"Use the new name"})
                .as_object()
                .unwrap()
                .clone(),
        )
        .await;
    assert!(
        matches!(outcome, Outcome::Failed { failure } if failure.code == code::PLUGIN_REJECTED)
    );
    node.kernel.shutdown().await.unwrap();
}
