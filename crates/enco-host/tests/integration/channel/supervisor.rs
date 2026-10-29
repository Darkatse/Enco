use super::*;
use enco_host::channel::{ChannelState, ChannelStatus, ChannelSupervisor};
use enco_kernel::{ChannelSettings, Loaded};
use tokio_util::sync::CancellationToken;

/// One identification, held until the test answers it.
struct OpenAttempt {
    base_url: String,
    generation: GenerationId,
    result: oneshot::Sender<Result<(), Fault>>,
}
impl OpenAttempt {
    /// Let the identification reach the account; the generation it opened with.
    fn succeed(self) -> GenerationId {
        self.result.send(Ok(())).unwrap();
        self.generation
    }
}
struct Mapper {
    adapter: Arc<ControlledChannel>,
    attempts: mpsc::UnboundedSender<OpenAttempt>,
}
#[async_trait]
impl enco_kernel::Channel for Mapper {
    async fn open(
        &self,
        settings: &ChannelSettings,
        generation: GenerationId,
        _store: Arc<dyn Store>,
    ) -> Result<Arc<dyn Adapter>, ChannelCallError> {
        let (result, completed) = oneshot::channel();
        self.attempts
            .send(OpenAttempt {
                base_url: settings.base_url.clone(),
                generation,
                result,
            })
            .unwrap();
        completed.await.unwrap()?;
        Ok(self.adapter.clone())
    }
}
fn loaded(mapper: Arc<Mapper>, provider: Arc<ScriptedProvider>) -> Loaded {
    let mut loaded = fixture(provider.clone(), provider);
    loaded.channel = Some(mapper);
    loaded
}
async fn apply(
    root: &Path,
    development: &enco_host::development::Development,
    registry: &enco_kernel::Registry,
    document: &Value,
) {
    std::fs::write(root.join("config.toml"), toml::to_string(document).unwrap()).unwrap();
    development
        .apply(
            registry,
            Initiator::Owner { reason: None },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
}
/// The statuses once `count` connections are configured and none is still opening.
async fn statuses(supervisor: &ChannelSupervisor, count: usize) -> Vec<ChannelStatus> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let statuses = supervisor.status().await.unwrap();
            if statuses.len() == count
                && statuses
                    .iter()
                    .all(|status| !matches!(status.state, ChannelState::Opening { .. }))
            {
                return statuses;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
fn state<'a>(statuses: &'a [ChannelStatus], name: &str) -> &'a ChannelState {
    &statuses
        .iter()
        .find(|status| status.name == name)
        .unwrap()
        .state
}
/// What identification reports while the protocol service refuses connections.
fn refused() -> Fault {
    Fault {
        failure: Failure {
            code: code::CHANNEL_NETWORK.into(),
            message: "connection refused".into(),
            retryable: true,
        },
        unknown: false,
        fatal: false,
        retry_after: None,
        origin: None,
    }
}

#[tokio::test]
async fn supervisor_follows_code_generations_after_settlement_and_has_one_owner_per_account() {
    let dir = tempfile::tempdir().unwrap();
    let Traffic {
        adapter,
        updates,
        mut polling,
        mut sending,
        ..
    } = traffic();
    let (attempts, mut openings) = mpsc::unbounded_channel();
    let mapper = Arc::new(Mapper { adapter, attempts });
    let provider = ScriptedProvider::new(vec![
        reply("answer"),
        reply("reply probe"),
        reply("compaction probe"),
    ]);
    let runtime = factory_runtime(loaded(mapper.clone(), provider.clone()));
    runtime.insert(b"replacement channel", loaded(mapper, provider));
    let mut document = configuration();
    document["channel"]["first"] =
        json!({"plugin": "fixture", "base_url": "https://fixture.invalid", "owner": "owner"});
    let (development, registry, store) =
        scripted_home(dir.path(), runtime, document.clone(), vec![]).await;
    let kernel = Arc::new(
        Kernel::start(kernel_deps(
            dir.path(),
            store.clone(),
            registry.clone(),
            None,
            Some(development.clone()),
        ))
        .await
        .unwrap(),
    );
    let supervisor = ChannelSupervisor::start(
        kernel.clone(),
        store.clone(),
        Arc::new(enco_host::SystemClock),
    );
    let original = receive(&mut openings).await.succeed();
    receive(&mut polling).await;
    updates
        .send(vec![update(1, Action::Message("first".into()))])
        .await
        .unwrap();
    receive(&mut polling).await;
    let delivery = receive(&mut sending).await;
    assert_eq!(delivery.text, "answer");
    source_fixture(
        dir.path(),
        &store,
        &development,
        &registry,
        "fixture",
        b"replacement channel",
    )
    .await;
    assert!(openings.try_recv().is_err());
    delivery.result.send(Ok(())).unwrap();
    let replacement = receive(&mut openings).await.succeed();
    assert_ne!(replacement, original);
    assert_eq!(receive(&mut polling).await, json!(1));
    document["channel"]["second"] = document["channel"]["first"].clone();
    apply(dir.path(), &development, &registry, &document).await;
    assert_eq!(receive(&mut openings).await.succeed(), replacement);
    let status = statuses(&supervisor, 2).await;
    assert!(matches!(state(&status, "first"), ChannelState::Running));
    assert!(
        matches!(state(&status, "second"), ChannelState::Duplicate { owner } if owner == "first")
    );
    document["channel"].as_object_mut().unwrap().remove("first");
    apply(dir.path(), &development, &registry, &document).await;
    assert_eq!(receive(&mut openings).await.succeed(), replacement);
    assert_eq!(receive(&mut polling).await, json!(1));
    assert!(matches!(
        statuses(&supervisor, 1).await[0].state,
        ChannelState::Running
    ));
    supervisor.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_retrying_opening_holds_up_no_other_connection_and_connects_once_the_service_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let Traffic {
        adapter,
        mut polling,
        updates: _updates,
        ..
    } = traffic();
    let (attempts, mut openings) = mpsc::unbounded_channel();
    let provider = ScriptedProvider::new(vec![]);
    let mapper = Arc::new(Mapper { adapter, attempts });
    let mut document = configuration();
    for name in ["offline", "ready"] {
        document["channel"][name] = json!({"plugin": "fixture", "base_url": format!("https://{name}.invalid"), "owner": "owner"});
    }
    let (development, registry, store) = scripted_home(
        dir.path(),
        factory_runtime(loaded(mapper, provider)),
        document.clone(),
        vec![],
    )
    .await;
    let kernel = Arc::new(
        Kernel::start(kernel_deps(
            dir.path(),
            store.clone(),
            registry.clone(),
            None,
            Some(development.clone()),
        ))
        .await
        .unwrap(),
    );
    let supervisor =
        ChannelSupervisor::start(kernel.clone(), store, Arc::new(enco_host::SystemClock));
    let first = receive(&mut openings).await;
    let second = receive(&mut openings).await;
    let (offline, ready) = if first.base_url.contains("offline") {
        (first, second)
    } else {
        (second, first)
    };
    offline.result.send(Err(refused())).unwrap();
    ready.succeed();
    receive(&mut polling).await;
    let retry = receive(&mut openings).await;
    assert!(retry.base_url.contains("offline"));
    let status = supervisor.status().await.unwrap();
    assert!(matches!(
        state(&status, "offline"),
        ChannelState::Opening { retrying: Some(failure) } if failure.reason.contains("connection refused")
    ));
    assert!(matches!(state(&status, "ready"), ChannelState::Running));

    // The account is free once its other connection is removed, while this one still retries.
    document["channel"].as_object_mut().unwrap().remove("ready");
    apply(dir.path(), &development, &registry, &document).await;
    assert!(!retry.result.is_closed());
    retry.succeed();
    receive(&mut polling).await;
    assert!(matches!(
        statuses(&supervisor, 1).await[0].state,
        ChannelState::Running
    ));
    supervisor.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_changed_connection_cancels_its_pending_identification_and_a_rejected_one_stops() {
    let dir = tempfile::tempdir().unwrap();
    let Traffic {
        adapter,
        updates: _updates,
        ..
    } = traffic();
    let (attempts, mut openings) = mpsc::unbounded_channel();
    let provider = ScriptedProvider::new(vec![]);
    let mapper = Arc::new(Mapper { adapter, attempts });
    let mut document = configuration();
    document["channel"]["only"] =
        json!({"plugin": "fixture", "base_url": "https://only.invalid", "owner": "owner"});
    let (development, registry, store) = scripted_home(
        dir.path(),
        factory_runtime(loaded(mapper, provider)),
        document.clone(),
        vec![],
    )
    .await;
    let kernel = Arc::new(
        Kernel::start(kernel_deps(
            dir.path(),
            store.clone(),
            registry.clone(),
            None,
            Some(development.clone()),
        ))
        .await
        .unwrap(),
    );
    let supervisor =
        ChannelSupervisor::start(kernel.clone(), store, Arc::new(enco_host::SystemClock));
    let pending = receive(&mut openings).await;
    document["channel"]["only"]["context"] = json!(1);
    apply(dir.path(), &development, &registry, &document).await;
    let replacement = receive(&mut openings).await;
    assert!(pending.result.is_closed());
    let mut rejected = refused();
    rejected.fatal = true;
    rejected.failure.retryable = false;
    rejected.failure.message = "credentials rejected".into();
    replacement.result.send(Err(rejected)).unwrap();
    assert!(matches!(
        &statuses(&supervisor, 1).await[0].state,
        ChannelState::Stopped { failure: Some(failure) } if failure.reason.contains("credentials rejected")
    ));
    assert!(openings.try_recv().is_err());
    supervisor.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
