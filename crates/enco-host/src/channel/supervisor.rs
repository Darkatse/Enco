//! Connections follow registered settings and usable mapper generations; an account has one owner.
use super::*;
use enco_kernel::{Store, View};

#[derive(Clone)]
struct Slot {
    settings: ConnectionSettings,
    route: Result<GenerationId, String>,
    current: Current,
}
#[derive(Clone)]
enum Current {
    Running(Arc<Channel>),
    Stopped {
        reason: String,
        origin: Option<MappingOrigin>,
    },
    Duplicate {
        key: String,
        owner: String,
    },
}

/// Maintains the configured connections and joins each old owner before replacing it.
pub struct ChannelSupervisor {
    current: Arc<Mutex<BTreeMap<String, Slot>>>,
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<Result<(), ChannelError>>>>,
}

impl ChannelSupervisor {
    /// Follow the registry's active view, opening accounts through their fixed mapper generation.
    pub fn start(kernel: Arc<Kernel>, store: Arc<dyn Store>, clock: Arc<dyn Clock>) -> Self {
        let mut views = kernel.registry().subscribe();
        let current = Arc::new(Mutex::new(BTreeMap::new()));
        let stop = CancellationToken::new();
        let task = tokio::spawn({
            let current = current.clone();
            let stop = stop.clone();
            async move {
                loop {
                    let view = views.borrow_and_update().clone();
                    if let Err(error) =
                        reconcile(&kernel, &store, &clock, &current, &view, &stop).await
                    {
                        shutdown(&current).await?;
                        return Err(error);
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return shutdown(&current).await,
                        result = views.changed() => if result.is_err() { return shutdown(&current).await; },
                    }
                }
            }
        });
        Self {
            current,
            stop,
            task: Mutex::new(Some(task)),
        }
    }

    /// Status for every configured connection, including account and menu failures.
    pub async fn status(&self) -> Result<Vec<ChannelStatus>, KernelError> {
        let current = lock(&self.current).clone();
        let mut statuses = Vec::new();
        for (name, slot) in current {
            statuses.push(match slot.current {
                Current::Running(channel) => channel.status().await?,
                Current::Stopped { reason, origin } => ChannelStatus {
                    name,
                    key: None,
                    running: false,
                    stopped: Some(reason),
                    failure_origin: origin,
                    menu_failure: None,
                    recent_failures: Vec::new(),
                },
                Current::Duplicate { key, owner } => ChannelStatus {
                    name,
                    stopped: Some(format!("account {key} is already owned by channel {owner}")),
                    key: Some(key),
                    running: false,
                    failure_origin: None,
                    menu_failure: None,
                    recent_failures: Vec::new(),
                },
            });
        }
        Ok(statuses)
    }

    /// Signal stop and wait until every connection and its helpers have stopped.
    pub async fn shutdown(&self) -> Result<(), ChannelError> {
        self.stop.cancel();
        let task = lock(&self.task).take();
        if let Some(task) = task {
            task.await.map_err(ChannelError::Unjoined)??;
        }
        Ok(())
    }
}
impl Drop for ChannelSupervisor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn reconcile(
    kernel: &Arc<Kernel>,
    store: &Arc<dyn Store>,
    clock: &Arc<dyn Clock>,
    current: &Mutex<BTreeMap<String, Slot>>,
    view: &View,
    stop: &CancellationToken,
) -> Result<(), ChannelError> {
    let settings = settings::read(&view.config.document)
        .map_err(|error| ChannelError::Config(error.to_string()))?;
    let routes: BTreeMap<_, _> = settings
        .iter()
        .map(|(name, setting)| {
            (
                name.clone(),
                view.exports
                    .channel(&setting.plugin)
                    .map(|export| export.generation)
                    .map_err(|error| error.message),
            )
        })
        .collect();
    let old = lock(current).clone();
    for (name, slot) in old {
        if settings.get(&name) != Some(&slot.settings) || routes.get(&name) != Some(&slot.route) {
            stop_slot(&slot).await?;
            lock(current).remove(&name);
        }
    }
    let mut accounts: BTreeMap<_, _> = lock(current)
        .iter()
        .filter_map(|(name, slot)| match &slot.current {
            Current::Running(channel) => Some((channel.key.clone(), name.clone())),
            Current::Stopped { .. } | Current::Duplicate { .. } => None,
        })
        .collect();
    let duplicates: Vec<_> = lock(current)
        .iter()
        .filter_map(|(name, slot)| match &slot.current {
            Current::Duplicate { key, .. } if !accounts.contains_key(key) => Some(name.clone()),
            _ => None,
        })
        .collect();
    for name in duplicates {
        lock(current).remove(&name);
    }
    for (name, settings) in settings {
        if stop.is_cancelled() {
            break;
        }
        if lock(current).contains_key(&name) {
            continue;
        }
        let route = routes[&name].clone();
        let result = async {
            let export = view.exports.channel(&settings.plugin).map_err(|error| ChannelError::Config(error.message))?;
            let transport = settings.transport();
            let adapter = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(Current::Stopped { reason: "connection stopped".into(), origin: None }),
                result = health::call(kernel, export.generation, export.adapter.open(&transport, export.generation, store.clone())) => result?,
            };
            let key = adapter.identity().key();
            if let Some(other) = accounts.get(&key) { return Ok(Current::Duplicate { key, owner: other.clone() }); }
            accounts.insert(key, name.clone());
            Ok::<_, ChannelError>(Current::Running(Arc::new(Channel::start(kernel.clone(), adapter, settings.clone(), export.generation, clock.clone()))))
        }.await;
        let current_slot = match result {
            Ok(current) => current,
            Err(error) => Current::Stopped {
                reason: error.to_string(),
                origin: error.origin(),
            },
        };
        lock(current).insert(
            name,
            Slot {
                settings,
                route,
                current: current_slot,
            },
        );
    }
    Ok(())
}

async fn stop_slot(slot: &Slot) -> Result<(), ChannelError> {
    if let Current::Running(channel) = &slot.current
        && let Err(error) = channel.shutdown().await
    {
        if matches!(error, ChannelError::Unjoined(_)) {
            return Err(error);
        }
        tracing::error!(%error, "previous channel owner stopped with an error");
    }
    Ok(())
}
async fn shutdown(current: &Mutex<BTreeMap<String, Slot>>) -> Result<(), ChannelError> {
    let snapshot = lock(current).clone();
    let mut failure = None;
    for (name, slot) in snapshot {
        if let Err(error) = stop_slot(&slot).await {
            failure.get_or_insert(error);
        }
        if let Some(slot) = lock(current).get_mut(&name) {
            slot.current = Current::Stopped {
                reason: "connection stopped".into(),
                origin: None,
            };
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
