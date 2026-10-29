#![expect(clippy::unwrap_used, reason = "behavior fixtures fail with context")]
use crate::support;

use enco_core::*;
use enco_host::{
    SqliteStore,
    development::*,
    status::{CallStep, Step},
};
use enco_kernel::*;
use std::{path::Path, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;

async fn home(root: &Path) -> (Arc<Development>, Arc<Registry>, Arc<SqliteStore>) {
    let provider = ScriptedProvider::new(vec![]);
    let runtime = factory_runtime(fixture(provider.clone(), provider.clone()));
    for bytes in [b"one".as_slice(), b"two", b"fixed"] {
        runtime.insert(bytes, fixture(provider.clone(), provider.clone()));
    }
    scripted_home(root, runtime, configuration(), vec![]).await
}

async fn apply_config(
    root: &Path,
    development: &Development,
    registry: &Registry,
    count: u32,
) -> Registered {
    let mut document = configuration();
    document["run"] = serde_json::json!({"max_rounds": count});
    std::fs::write(
        root.join("config.toml"),
        format!(
            "# Keep owner comments\n{}",
            toml::to_string(&document).unwrap()
        ),
    )
    .unwrap();
    development
        .apply(
            registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .registered
        .unwrap()
}

async fn rewind(
    development: &Development,
    registry: &Registry,
    registration: RegistrationId,
    paths: Option<Vec<IntentPath>>,
) -> Result<Rewound, Failed> {
    development
        .rewind(
            registry,
            RewindArguments {
                registration,
                paths,
            },
            &CancellationToken::new(),
        )
        .await
}

/// Registering the edits makes the same rewind pass; overwriting covers every selected path.
fn assert_refused_steps(failed: &Failed, baseline: RegistrationId) {
    let next = failed.next();
    assert_eq!(
        next.first,
        [
            Step::Call(CallStep::Register),
            Step::Call(CallStep::Rewind(RewindArguments {
                registration: baseline,
                paths: None,
            })),
        ]
    );
    assert_eq!(
        next.alternative,
        Some(vec![Step::Call(CallStep::Overwrite(RewindArguments {
            registration: baseline,
            paths: Some(vec![
                IntentPath::Configuration,
                "plugins/alpha".parse().unwrap()
            ]),
        }))])
    );
}

#[tokio::test]
async fn rewind_checks_only_files_it_will_overwrite_and_preserves_unselected_units() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let (development, registry, store) = home(root).await;
    let directory = root.join("plugins/alpha");
    std::fs::create_dir_all(directory.join(".cargo")).unwrap();
    std::fs::write(directory.join(".gitignore"), "/target/\n").unwrap();
    std::fs::write(directory.join(".cargo/config.toml"), "# first\n").unwrap();
    let first = source_fixture(root, &store, &development, &registry, "alpha", b"one").await;
    source_fixture(root, &store, &development, &registry, "unchanged", b"one").await;
    let baseline = apply_config(root, &development, &registry, 20)
        .await
        .registration
        .id;
    let config = std::fs::read(root.join("config.toml")).unwrap();
    std::fs::write(directory.join(".cargo/config.toml"), "# second\n").unwrap();
    source_fixture(root, &store, &development, &registry, "alpha", b"two").await;
    apply_config(root, &development, &registry, 21).await;
    let changed = std::fs::read(root.join("config.toml")).unwrap();
    std::fs::write(directory.join("source.txt"), b"owner edit").unwrap();
    std::fs::write(
        root.join("plugins/unchanged/source.txt"),
        b"leave this edit",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("plugins/draft")).unwrap();
    std::fs::write(root.join("plugins/draft/new.txt"), b"unregistered").unwrap();
    let failed = rewind(&development, &registry, baseline, None)
        .await
        .err()
        .unwrap();
    assert!(
        matches!(&*failed.error, DevelopmentError::RewindRefused { conflicts, .. }
        if matches!(conflicts.as_slice(), [RewindConflict { path, conflicts }]
            if *path == "plugins/alpha".parse().unwrap()
                && matches!(conflicts.as_slice(), [tree::WriteConflict::Unregistered { path }]
                    if *path == directory.join("source.txt"))))
    );
    assert_refused_steps(&failed, baseline);
    assert_eq!(std::fs::read(root.join("config.toml")).unwrap(), changed);
    rewind(
        &development,
        &registry,
        baseline,
        Some(vec!["plugins/alpha".parse().unwrap()]),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(directory.join(".cargo/config.toml")).unwrap(),
        b"# first\n"
    );
    let modified = std::fs::metadata(directory.join(".cargo/config.toml"))
        .unwrap()
        .modified()
        .unwrap();
    // Retrying the default selection neither conflicts with nor rewrites files already restored.
    let result = rewind(&development, &registry, baseline, None)
        .await
        .unwrap();
    assert_eq!(result.written, ["config.toml".parse().unwrap()]);
    assert_eq!(result.unchanged, ["plugins/alpha".parse().unwrap()]);
    assert_eq!(std::fs::read(root.join("config.toml")).unwrap(), config);
    assert_eq!(std::fs::read(directory.join("source.txt")).unwrap(), b"one");
    assert_eq!(
        std::fs::metadata(directory.join(".cargo/config.toml"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    assert_eq!(
        std::fs::read(root.join("plugins/unchanged/source.txt")).unwrap(),
        b"leave this edit"
    );
    assert_eq!(
        std::fs::read(root.join("plugins/draft/new.txt")).unwrap(),
        b"unregistered"
    );
    let status = registry.status().await.unwrap();
    let alpha = status
        .plugins
        .iter()
        .find(|plugin| plugin.id == first.generations[0].plugin)
        .unwrap();
    assert_ne!(
        running_generation(alpha).unwrap().artifact,
        first.generations[0].artifact
    );
}

#[tokio::test]
async fn rewind_follows_identity_through_rename() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, store) = home(root.path()).await;
    let first = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"one",
    )
    .await;
    let cancel = CancellationToken::new();
    development
        .rename(
            &registry,
            &"alpha".parse().unwrap(),
            &"beta".parse().unwrap(),
            Initiator::Owner { reason: None },
            &cancel,
        )
        .await
        .unwrap();
    let renamed = rewind(&development, &registry, first.registration.id, None)
        .await
        .unwrap();
    assert!(renamed.written.is_empty() && renamed.unchanged.is_empty());
    source_fixture(root.path(), &store, &development, &registry, "beta", b"two").await;
    let preview = development
        .history(
            &registry,
            HistoryQuery::Compare {
                from: Some(HistoryPoint::Home),
                to: HistoryPoint::Registration(first.registration.id),
            },
        )
        .await
        .unwrap()
        .comparison
        .unwrap();
    assert_eq!(preview.units[0].path, "plugins/beta".parse().unwrap());
    assert!(
        matches!(preview.next.unwrap().first.as_slice(), [Step::Call(CallStep::Rewind(args))] if args.paths.as_ref().unwrap() == &["plugins/beta".parse().unwrap()])
    );
    let result = rewind(&development, &registry, first.registration.id, None)
        .await
        .unwrap();
    assert_eq!(result.written, ["plugins/beta".parse().unwrap()]);
    assert!(!root.path().join("plugins/alpha").exists());
    assert_eq!(
        std::fs::read(root.path().join("plugins/beta/source.txt")).unwrap(),
        b"one"
    );
    assert_eq!(
        registry.registered_intent(None).await.unwrap().plugins["beta"].id,
        first.generations[0].plugin
    );
    source_fixture(root.path(), &store, &development, &registry, "beta", b"one").await;
    let directory = root.path().join("plugins/beta");
    std::fs::write(
        directory.join("source.txt"),
        b"unregistered after restoring",
    )
    .unwrap();
    let reverted = rewind(&development, &registry, first.registration.id, None)
        .await
        .unwrap();
    assert!(reverted.written.is_empty() && reverted.unchanged.is_empty());
    assert_eq!(
        std::fs::read(directory.join("source.txt")).unwrap(),
        b"unregistered after restoring"
    );
}

#[tokio::test]
async fn rewind_refuses_to_remove_ignored_content() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, store) = home(root.path()).await;
    let initial = registry
        .registered_intent(None)
        .await
        .unwrap()
        .registration()
        .unwrap();
    source_fixture(root.path(), &store, &development, &registry, "beta", b"one").await;
    let directory = root.path().join("plugins/beta");
    std::fs::create_dir(directory.join(".git")).unwrap();
    std::fs::write(directory.join(".git/owner"), b"keep").unwrap();
    let paths = Some(vec!["plugins/beta".parse().unwrap()]);
    let failed = rewind(&development, &registry, initial, paths.clone())
        .await
        .err()
        .unwrap();
    assert!(
        matches!(&*failed.error, DevelopmentError::RewindRefused { conflicts, .. }
        if conflicts.iter().any(|unit| unit.conflicts.iter().any(|conflict|
            matches!(conflict, tree::WriteConflict::Ignored { path } if *path == directory.join(".git")))))
    );
    assert!(directory.join("source.txt").exists());
    std::fs::remove_dir_all(directory.join(".git")).unwrap();
    let result = rewind(&development, &registry, initial, paths)
        .await
        .unwrap();
    assert_eq!(result.removed, ["plugins/beta".parse().unwrap()]);
    assert!(!directory.exists());
}

#[tokio::test]
async fn history_reports_missing_content_locally_and_rewind_preflights_before_writing() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, store) = home(root.path()).await;
    let first = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"one",
    )
    .await;
    source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"two",
    )
    .await;
    apply_config(root.path(), &development, &registry, 22).await;
    let config = std::fs::read(root.path().join("config.toml")).unwrap();
    std::fs::remove_file(store.blob_path(&ContentHash::of(b"one"))).unwrap();
    let history = development
        .history(
            &registry,
            HistoryQuery::Compare {
                from: Some(HistoryPoint::Registration(first.registration.id)),
                to: HistoryPoint::Home,
            },
        )
        .await
        .unwrap();
    let units = history.comparison.unwrap().units;
    assert!(
        units
            .iter()
            .any(|unit| unit.path == "config.toml".parse().unwrap() && unit.files.iter().any(|file| matches!(&file.change, FileDifference::Text { diff } if diff.contains("max_rounds"))))
    );
    let missing = units
        .iter()
        .find(|unit| unit.path == "plugins/alpha".parse().unwrap())
        .unwrap()
        .unavailable
        .as_ref()
        .unwrap();
    assert_eq!(
        missing.point,
        HistoryPoint::Registration(first.registration.id)
    );
    assert!(
        matches!(missing.cause, ContentFailure::Missing { hash } if hash == ContentHash::of(b"one"))
    );
    let failed = rewind(&development, &registry, first.registration.id, None)
        .await
        .err()
        .unwrap();
    assert!(
        matches!(&*failed.error, DevelopmentError::RewindContent { path, unavailable }
        if *path == "plugins/alpha".parse().unwrap()
            && unavailable.point == HistoryPoint::Registration(first.registration.id)
            && matches!(unavailable.cause, ContentFailure::Missing { hash } if hash == ContentHash::of(b"one")))
    );
    assert_eq!(
        std::fs::read(root.path().join("config.toml")).unwrap(),
        config
    );
    assert_eq!(
        std::fs::read(root.path().join("plugins/alpha/source.txt")).unwrap(),
        b"two"
    );
}

#[tokio::test]
async fn a_generation_replaced_during_its_trial_stays_unpromoted_and_inactive() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, store) = home(root.path()).await;
    let selected = |changes: &[UnitChange]| match changes {
        [
            ..,
            UnitChange::Plugin {
                change: PluginChangeKind::Code { generation },
                ..
            },
        ] => (generation.record.status, generation.active),
        _ => panic!("the registration selected no code"),
    };
    let first = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"one",
    )
    .await;
    assert_eq!(selected(&first.changes), (GenerationStatus::Trial, true));
    source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"two",
    )
    .await;
    let earlier = registry.registration(first.registration.id).await.unwrap();
    assert_eq!(selected(&earlier.changes), (GenerationStatus::Trial, false));
}

#[tokio::test]
async fn history_pages_provenance_and_status_bounds_are_independent_of_history_length() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, store) = home(root.path()).await;
    for count in 1..=HISTORY_PAGE_SIZE + 1 {
        apply_config(
            root.path(),
            &development,
            &registry,
            u32::try_from(count).unwrap(),
        )
        .await;
    }
    let reason = "Use the owner timezone";
    let actor = Initiator::Agent {
        session: store.ensure_session("main", Utc::now()).await.unwrap().id,
        call: CallId::new(),
        reason: reason.parse().unwrap(),
    };
    let mut document = configuration();
    document["owner"]["timezone"] = "Europe/Paris".into();
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    let applied = development
        .apply(&registry, actor.clone(), &CancellationToken::new())
        .await
        .unwrap()
        .registered
        .unwrap();
    let page = development
        .history(&registry, HistoryQuery::List { before: None })
        .await
        .unwrap();
    assert_eq!(page.registrations.len(), HISTORY_PAGE_SIZE);
    assert_eq!(page.registrations[0].id, applied.registration.id);
    assert!(matches!((&page.registrations[0].initiator, &actor),
        (Initiator::Agent { session, call, reason }, Initiator::Agent { session: id, call: expected_call, reason: expected_reason })
        if session.id == *id && session.name == "main" && call == expected_call && reason == expected_reason));
    assert_eq!(
        page.registrations[0].initiator.reason().map(Reason::as_str),
        Some(reason)
    );
    assert!(page.registrations[0].changes.iter().any(|change| matches!(change, UnitChange::Configuration { sections, .. } if sections == &["owner", "run"])));
    let older = development
        .history(
            &registry,
            HistoryQuery::List {
                before: page.before,
            },
        )
        .await
        .unwrap();
    assert_eq!(older.registrations.len(), 3);
    assert!(older.before.is_none());
    let detail = development
        .history(
            &registry,
            HistoryQuery::Compare {
                from: None,
                to: HistoryPoint::Registration(applied.registration.id),
            },
        )
        .await
        .unwrap();
    assert_eq!(detail.registrations.len(), 1);
    assert!(
        detail.comparison.unwrap().units[0].files.iter().any(|file| matches!(&file.change, FileDifference::Text { diff } if diff.contains("Europe/Paris")))
    );
    let status = development.status(&registry).await.unwrap();
    assert_eq!(status.recent_registrations.len(), RECENT_REGISTRATIONS);
    assert!(matches!(status.configuration.home, HomeComparison::Same));
    assert_eq!(
        status.configuration.running.registration,
        applied.registration.id
    );
}

#[tokio::test]
async fn large_history_results_keep_a_unit_summary_and_save_the_full_diff() {
    let root = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new(vec![
        calls(vec![call("self_history", r#"{"to":"home"}"#)]),
        reply("reviewed"),
    ]);
    let runtime = factory_runtime(fixture(provider.clone(), provider));
    let (development, registry, store) =
        scripted_home(root.path(), runtime, configuration(), vec![]).await;
    std::fs::create_dir_all(root.path().join("plugins/draft")).unwrap();
    let lines: String = (0..10_000)
        .map(|index| format!("line-{index}: draft content for review\n"))
        .collect();
    std::fs::write(root.path().join("plugins/draft/source.txt"), lines).unwrap();
    std::fs::write(root.path().join("plugins/draft/empty"), b"").unwrap();
    let kernel = Kernel::start(kernel_deps(
        root.path(),
        store.clone(),
        registry,
        None,
        Some(development),
    ))
    .await
    .unwrap();
    let session = kernel.open_session("review").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    kernel
        .submit(
            session.id,
            EventId::new(),
            "review pending intent".into(),
            Vec::new(),
        )
        .await
        .unwrap();
    let log = finish(&mut rx).await;
    let (preview, full) = log
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled {
                outcome: Settlement::Ok,
                content,
                full,
                ..
            } => Some((content, full.unwrap())),
            _ => None,
        })
        .unwrap();
    assert!(preview.contains("plugins/draft"));
    let full = String::from_utf8(store.get_blob(&full).await.unwrap()).unwrap();
    assert!(full.contains("line-9999") && full.contains("plugins/draft/empty"));
    assert!(preview.len() < full.len());
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn rewind_refuses_ignore_rule_changes_before_writing_any_selected_path() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, store) = home(root.path()).await;
    let directory = root.path().join("plugins/alpha");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join(".gitignore"), "/target/\n").unwrap();
    let first = source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"one",
    )
    .await;
    std::fs::write(directory.join(".gitignore"), "/target/\nprivate\n").unwrap();
    source_fixture(
        root.path(),
        &store,
        &development,
        &registry,
        "alpha",
        b"two",
    )
    .await;
    std::fs::write(directory.join("private"), b"owner bytes").unwrap();
    let failed = rewind(&development, &registry, first.registration.id, None)
        .await
        .err()
        .unwrap();
    assert!(matches!(
        *failed.error,
        DevelopmentError::RewindRefused { .. }
    ));
    assert_eq!(
        std::fs::read(directory.join("private")).unwrap(),
        b"owner bytes"
    );
    assert_eq!(std::fs::read(directory.join("source.txt")).unwrap(), b"two");
    assert_eq!(
        std::fs::read(directory.join(".gitignore")).unwrap(),
        b"/target/\nprivate\n"
    );
}

#[tokio::test]
async fn configuration_rewind_preserves_an_existing_file_link() {
    let root = tempfile::tempdir().unwrap();
    let (development, registry, _) = home(root.path()).await;
    let initial = registry
        .registered_intent(None)
        .await
        .unwrap()
        .registration()
        .unwrap();
    let original = std::fs::read(root.path().join("config.toml")).unwrap();
    apply_config(root.path(), &development, &registry, 30).await;
    std::fs::rename(
        root.path().join("config.toml"),
        root.path().join("owner.toml"),
    )
    .unwrap();
    std::os::unix::fs::symlink("owner.toml", root.path().join("config.toml")).unwrap();
    let cancel = CancellationToken::new();
    assert!(
        development
            .apply(&registry, Initiator::Owner { reason: None }, &cancel)
            .await
            .unwrap()
            .registered
            .is_none()
    );
    rewind(&development, &registry, initial, None)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_link(root.path().join("config.toml")).unwrap(),
        Path::new("owner.toml")
    );
    assert_eq!(
        std::fs::read(root.path().join("owner.toml")).unwrap(),
        original
    );
}
