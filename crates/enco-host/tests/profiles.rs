#![expect(
    clippy::unwrap_used,
    reason = "test provider and configuration helpers fail the test by panicking"
)]
mod support;

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
        api_key: Option<&str>,
        request: ProviderRequest,
    ) -> Result<Completion, Failure> {
        if settings.model == "old-compaction" && self.pause.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        self.provider.complete(settings, api_key, request).await
    }
}

fn configure(deps: &mut KernelDeps) {
    let old = deps.profiles.get_mut("default").unwrap();
    old.reply.settings.model = "old-reply".into();
    old.reply.budget = Budget {
        context_tokens: 6000,
        max_output_tokens: 256,
    };
    old.compaction.settings.model = "old-compaction".into();
    old.compaction.budget.max_output_tokens = 512;
    let mut alternate = profile();
    alternate.reply.settings.model = "new-reply".into();
    alternate.compaction.settings.model = "new-compaction".into();
    deps.profiles.insert("alternate".into(), alternate);
}

#[tokio::test]
async fn profile_changes_bind_at_round_boundaries_and_missing_profiles_remain_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = ScriptedProvider::new((0..30).map(|_| reply("summary or reply")).collect());
    let provider = Arc::new(PausedCompaction {
        provider: scripted.clone(),
        pause: AtomicBool::new(true),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), configure).await;
    let main = kernel.open_session("main").await.unwrap();
    let other = kernel.open_session("other").await.unwrap();
    let mut rx = kernel.subscribe(main.id).unwrap();
    let conversation = async {
        for n in 0..6 {
            kernel
                .submit(
                    main.id,
                    EventId::new(),
                    format!("History {n}: {}", "some detailed history ".repeat(150)),
                )
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
        let log = kernel.log(main.id, None).await.unwrap();
        let round = log
            .iter()
            .rev()
            .find_map(|entry| match entry.body {
                EntryBody::AttemptStarted { round, .. } => Some(round),
                _ => None,
            })
            .unwrap();
        kernel.set_profile(main.id, "alternate").await.unwrap();
        let status = kernel.status().await.unwrap();
        assert_eq!(
            status
                .sessions
                .iter()
                .find(|s| s.session.id == main.id)
                .unwrap()
                .session
                .profile,
            "alternate"
        );
        provider.resume.notify_one();
        round
    };
    let (_, round) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(conversation, switch)
    })
    .await
    .unwrap();
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
        .submit(other.id, EventId::new(), "independent profile".into())
        .await
        .unwrap();
    finish(&mut other_rx).await;
    assert_eq!(
        kernel.inspect(other.id, None).await.unwrap().settings.model,
        "old-reply"
    );
    assert_eq!(
        kernel.inspect(main.id, None).await.unwrap().settings.model,
        "new-reply"
    );
    kernel.shutdown().await.unwrap();
    drop(kernel);

    // Removing a configured name affects its Sessions, not startup or other owners.
    let (kernel, _) = support::kernel(dir.path(), scripted.clone()).await;
    let mut rx = kernel.subscribe(main.id).unwrap();
    kernel
        .submit(main.id, EventId::new(), "continue".into())
        .await
        .unwrap();
    assert!(matches!(&finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded { end: RunEnd::Failed { failure }, .. } if failure.code == code::PROFILE_UNKNOWN));
    let mut other_rx = kernel.subscribe(other.id).unwrap();
    kernel
        .submit(other.id, EventId::new(), "still works".into())
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
        .submit(main.id, EventId::new(), "recovered".into())
        .await
        .unwrap();
    assert!(matches!(
        finish(&mut rx).await.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    assert!(
        scripted.requests.lock().unwrap().last().unwrap().messages[0]
            .joined_text()
            .contains("unknown profile alternate")
    );
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn history_too_large_for_the_compaction_window_fails_only_when_the_reply_no_longer_fits() {
    let dir = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::new((0..10).map(|_| reply("reply")).collect());
    let (kernel, _) = kernel_with(dir.path(), provider.clone(), |deps| {
        configure(deps);
        deps.profiles.get_mut("default").unwrap().compaction.budget = Budget {
            context_tokens: 100,
            max_output_tokens: 10,
        };
    })
    .await;
    let session = kernel.open_session("main").await.unwrap();
    let mut rx = kernel.subscribe(session.id).unwrap();
    let mut overflow = None;
    for _ in 0..10 {
        kernel
            .submit(
                session.id,
                EventId::new(),
                "some detailed history ".repeat(150),
            )
            .await
            .unwrap();
        let entries = finish(&mut rx).await;
        if let EntryBody::RunEnded {
            end: RunEnd::Failed { failure },
            ..
        } = &entries.last().unwrap().body
        {
            overflow = Some(failure.clone());
            break;
        }
    }
    let failure = overflow.expect("history must outgrow the reply window");
    assert_eq!(failure.code, code::CONTEXT_OVERFLOW);
    assert!(failure.message.contains("window is 6000"));
    // No boundary fits a 100-token window, so no summary request is ever sent.
    assert!(
        provider
            .settings
            .lock()
            .unwrap()
            .iter()
            .all(|s| s.model == "old-reply")
    );
    kernel.shutdown().await.unwrap();
}
