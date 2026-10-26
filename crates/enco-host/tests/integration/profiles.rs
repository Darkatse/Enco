#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
use crate::support;

use async_trait::async_trait;
use enco_core::*;
use enco_kernel::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::*;
use tokio::sync::Notify;

struct PausedCompaction {
    provider: Arc<ScriptedProvider>,
    pause: AtomicBool,
    entered: Notify,
    resume: Notify,
}

#[async_trait]
impl Provider for PausedCompaction {
    async fn complete(
        &self,
        settings: &ProviderSettings,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        if settings.model == "old-compaction" && self.pause.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        self.provider.complete(settings, request).await
    }
}

fn configured() -> serde_json::Value {
    let mut document = configuration();
    document["endpoint"]["alternate-reply"] = document["endpoint"]["reply"].clone();
    document["endpoint"]["alternate-compaction"] = document["endpoint"]["compaction"].clone();
    document["endpoint"]["reply"]["model"] = "old-reply".into();
    document["endpoint"]["reply"]["window_tokens"] = 6000.into();
    document["endpoint"]["reply"]["max_output_tokens"] = 256.into();
    document["endpoint"]["compaction"]["model"] = "old-compaction".into();
    document["endpoint"]["compaction"]["max_output_tokens"] = 512.into();
    document["endpoint"]["alternate-reply"]["model"] = "new-reply".into();
    document["endpoint"]["alternate-compaction"]["model"] = "new-compaction".into();
    document["profile"]["alternate"] =
        serde_json::json!({"reply": ["alternate-reply"], "compaction": ["alternate-compaction"]});
    document
}

/// Starts a compaction on `main` and, while it is in flight, either
/// assigns `main` the alternate profile or redefines its default one; returns that Round.
async fn change_profile_during_compaction(
    kernel: &Kernel,
    provider: &PausedCompaction,
    main: SessionId,
    update_definition: bool,
) -> RoundId {
    let mut rx = kernel.subscribe(main).unwrap();
    let conversation = async {
        for n in 0..2 {
            kernel
                .submit(main, EventId::new(), format!("History {n}"), Vec::new())
                .await
                .unwrap();
            assert!(matches!(
                finish(&mut rx).await.last().unwrap().body,
                EntryBody::RunEnded {
                    end: RunEnd::Completed,
                    ..
                }
            ));
        }
    };
    let switch = async {
        provider.entered.notified().await;
        let log = kernel.log(main, None).await.unwrap();
        let round = log
            .iter()
            .rev()
            .find_map(|entry| match entry.body {
                EntryBody::AttemptStarted { round, .. } => Some(round),
                _ => None,
            })
            .unwrap();
        if update_definition {
            let mut document = configured();
            document["profile"]["default"] = document["profile"]["alternate"].clone();
            submit(kernel.registry(), configuration_request(document))
                .await
                .unwrap();
        } else {
            kernel.set_profile(main, "alternate").await.unwrap();
        }
        let status = kernel.status().await.unwrap();
        assert_eq!(
            status
                .sessions
                .iter()
                .find(|s| s.session.id == main)
                .unwrap()
                .session
                .profile,
            if update_definition {
                "default"
            } else {
                "alternate"
            }
        );
        provider.resume.notify_one();
        round
    };
    let (_, round) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(conversation, switch)
    })
    .await
    .unwrap();
    round
}

#[tokio::test]
async fn profile_changes_bind_at_round_boundaries() {
    for update_definition in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let scripted = ScriptedProvider::new((0..30).map(|_| reply("summary or reply")).collect());
        scripted
            .steps
            .lock()
            .unwrap()
            .push_front(calls(vec![call("fs_list", r#"{"path":"."}"#)]));
        let provider = Arc::new(PausedCompaction {
            provider: scripted.clone(),
            pause: AtomicBool::new(true),
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let (kernel, _) = kernel_configured_composer(
            dir.path(),
            provider.clone(),
            configured(),
            vec![],
            Arc::new(HistoryComposer),
            |_| {},
        )
        .await;
        let main = kernel.open_session("main").await.unwrap();
        let other = kernel.open_session("other").await.unwrap();
        let round =
            change_profile_during_compaction(&kernel, &provider, main.id, update_definition).await;
        let log = kernel.log(main.id, None).await.unwrap();
        let models: Vec<_> = log
            .iter()
            .filter_map(|entry| match &entry.body {
                EntryBody::AttemptStarted {
                    round: id,
                    settings,
                    ..
                } if *id == round => Some(settings.model.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(models, ["old-compaction", "old-reply"]);
        assert_eq!(
            scripted.settings.lock().unwrap().last().unwrap().model,
            "new-reply"
        );
        let mut other_rx = kernel.subscribe(other.id).unwrap();
        kernel
            .submit(
                other.id,
                EventId::new(),
                "independent profile".into(),
                Vec::new(),
            )
            .await
            .unwrap();
        finish(&mut other_rx).await;
        assert_eq!(
            kernel.inspect(other.id, None).await.unwrap().settings.model,
            if update_definition {
                "new-reply"
            } else {
                "old-reply"
            }
        );
        assert_eq!(
            kernel.inspect(main.id, None).await.unwrap().settings.model,
            "new-reply"
        );
        kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_session_whose_profile_was_rolled_back_away_remains_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = ScriptedProvider::new((0..30).map(|_| reply("summary or reply")).collect());
    let (kernel, _) =
        kernel_configured(dir.path(), scripted.clone(), configured(), vec![], |_| {}).await;
    let mut earlier = configured();
    earlier["profile"]
        .as_object_mut()
        .unwrap()
        .remove("alternate");
    submit(kernel.registry(), configuration_request(earlier))
        .await
        .unwrap();
    submit(kernel.registry(), configuration_request(configured()))
        .await
        .unwrap();
    let main = kernel.open_session("main").await.unwrap();
    let other = kernel.open_session("other").await.unwrap();
    kernel.set_profile(main.id, "alternate").await.unwrap();
    kernel.registry().rollback().await.unwrap();
    kernel.shutdown().await.unwrap();
    drop(kernel);

    // Mechanical rollback can remove an in-use name; restarting still restores that generation.
    let (kernel, _) = support::kernel(dir.path(), scripted.clone()).await;
    let mut rx = kernel.subscribe(main.id).unwrap();
    kernel
        .submit(main.id, EventId::new(), "continue".into(), Vec::new())
        .await
        .unwrap();
    assert!(matches!(&finish(&mut rx).await.last().unwrap().body,
    EntryBody::RunEnded { end: RunEnd::Failed { failure }, .. } if failure.code == code::PROFILE_UNKNOWN));
    let mut other_rx = kernel.subscribe(other.id).unwrap();
    kernel
        .submit(other.id, EventId::new(), "still works".into(), Vec::new())
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut other_rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    assert!(matches!(
        kernel.set_profile(main.id, "missing").await,
        Err(KernelError::UnknownProfile(_))
    ));
    kernel.set_profile(main.id, "default").await.unwrap();
    kernel
        .submit(main.id, EventId::new(), "recovered".into(), Vec::new())
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn configuration_admission_observes_profile_assignments_made_during_probing() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(PausedCompaction {
        provider: ScriptedProvider::new(vec![reply("OK"), reply("OK")]),
        pause: AtomicBool::new(true),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let (kernel, _) =
        kernel_configured(dir.path(), provider.clone(), configured(), vec![], |_| {}).await;
    let session = kernel.open_session("main").await.unwrap();
    let before = kernel.registry().view().config.generation;
    let mut document = configured();
    document["profile"]
        .as_object_mut()
        .unwrap()
        .remove("alternate");
    document["endpoint"]["reply"]["model"] = "old-compaction".into();
    let apply = submit(kernel.registry(), configuration_request(document.clone()));
    let assign = async {
        provider.entered.notified().await;
        kernel.set_profile(session.id, "alternate").await.unwrap();
        provider.resume.notify_one();
    };
    let (result, ()) = tokio::join!(apply, assign);
    let Err(RegistryError::Rejected(causes)) = result else {
        panic!("expected profile rejection");
    };
    assert!(
        matches!(causes.as_slice(), [Cause::ProfileInUse { profile, sessions }]
            if profile == "alternate" && sessions.iter().eq([&session.name])),
        "{causes:?}"
    );
    assert_eq!(kernel.registry().view().config.generation, before);
    kernel.set_profile(session.id, "default").await.unwrap();
    submit(kernel.registry(), configuration_request(document))
        .await
        .unwrap();
    assert!(matches!(
        kernel.set_profile(session.id, "alternate").await,
        Err(KernelError::UnknownProfile(_))
    ));
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn configuration_probes_conflict_with_registration_but_not_trial_promotion() {
    for promote in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(PausedCompaction {
            provider: ScriptedProvider::new(vec![reply("OK")]),
            pause: AtomicBool::new(true),
            entered: Notify::new(),
            resume: Notify::new(),
        });
        let (kernel, _) =
            kernel_configured(dir.path(), provider.clone(), configured(), vec![], |_| {}).await;
        let registry = kernel.registry();
        let trial = submit(registry, source_request("fixture", FACTORY_BYTES))
            .await
            .unwrap()
            .generations
            .remove(0)
            .id;
        let before = registry.view().config.generation;
        let mut document = configured();
        document["endpoint"]["reply"]["model"] = "old-compaction".into();
        let apply = submit(registry, configuration_request(document));
        let change = async {
            provider.entered.notified().await;
            if promote {
                for _ in 0..TRIAL_CALLS {
                    registry.report(trial, Verdict::Ok, None).await.unwrap();
                }
            } else {
                submit(registry, source_request("fixture", FACTORY_BYTES))
                    .await
                    .unwrap();
            }
            provider.resume.notify_one();
        };
        let (result, ()) = tokio::join!(apply, change);
        if promote {
            assert!(result.is_ok());
            assert!(registry.view().config.generation > before);
        } else {
            assert!(matches!(result, Err(RegistryError::Conflict(_))));
            assert_eq!(registry.view().config.generation, before);
        }
        kernel.shutdown().await.unwrap();
    }
}
