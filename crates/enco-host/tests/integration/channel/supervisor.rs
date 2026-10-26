use super::*;
use enco_host::channel::ChannelSupervisor;
use enco_kernel::{ChannelSettings, Loaded};
use tokio_util::sync::CancellationToken;

struct Mapper {
    adapter: Arc<ControlledChannel>,
    opened: mpsc::UnboundedSender<GenerationId>,
}
#[async_trait]
impl enco_kernel::Channel for Mapper {
    async fn open(
        &self,
        _settings: &ChannelSettings,
        generation: GenerationId,
        _store: Arc<dyn Store>,
    ) -> Result<Arc<dyn Adapter>, ChannelCallError> {
        self.opened.send(generation).unwrap();
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
async fn statuses(
    supervisor: &ChannelSupervisor,
    count: usize,
) -> Vec<enco_host::channel::ChannelStatus> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let statuses = supervisor.status().await.unwrap();
            if statuses.len() == count {
                return statuses;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
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
    let (opened, mut openings) = mpsc::unbounded_channel();
    let mapper = Arc::new(Mapper { adapter, opened });
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
    let original = receive(&mut openings).await;
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
    let replacement = receive(&mut openings).await;
    assert_ne!(replacement, original);
    assert_eq!(receive(&mut polling).await, json!(1));
    document["channel"]["second"] = document["channel"]["first"].clone();
    apply(dir.path(), &development, &registry, &document).await;
    assert_eq!(receive(&mut openings).await, replacement);
    let status = statuses(&supervisor, 2).await;
    assert_eq!(status.iter().filter(|status| status.running).count(), 1);
    assert!(status.iter().any(|status| {
        status
            .stopped
            .as_deref()
            .is_some_and(|reason| reason.contains("controlled:bot") && reason.contains("first"))
    }));
    document["channel"].as_object_mut().unwrap().remove("first");
    apply(dir.path(), &development, &registry, &document).await;
    assert_eq!(receive(&mut openings).await, replacement);
    assert_eq!(receive(&mut polling).await, json!(1));
    assert!(statuses(&supervisor, 1).await[0].running);
    supervisor.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();
}
