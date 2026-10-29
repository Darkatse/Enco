//! Connections follow registered settings and usable mapper generations; an account has one owner.
//!
//! Each account is identified in its own task, the way a connection runs its activity requests:
//! the supervisor starts it, may stop it at any time, and admits what it returns.
use super::*;
mod opening;

use enco_kernel::{Store, View};
use opening::{Opened, Opening};
use std::collections::BTreeSet;
use tokio::{sync::watch, task::JoinSet};

#[derive(Clone)]
struct Slot {
    settings: ConnectionSettings,
    route: Result<GenerationId, String>,
    current: Current,
}
#[derive(Clone)]
enum Current {
    Opening(Arc<Opening>),
    Running(Arc<Channel>),
    Stopped(Option<ChannelFailure>),
    Duplicate { key: String, owner: String },
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
        let views = kernel.registry().subscribe();
        let current = Arc::new(Mutex::new(BTreeMap::new()));
        let stop = CancellationToken::new();
        let mut supervision = Supervision {
            kernel,
            store,
            clock,
            current: current.clone(),
            stop: stop.clone(),
            openings: JoinSet::new(),
        };
        let task = tokio::spawn(async move {
            let result = supervision.run(views).await;
            let stopped = supervision.shutdown().await;
            result.and(stopped)
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
            let (key, state) = match slot.current {
                Current::Running(channel) => {
                    statuses.push(channel.status().await?);
                    continue;
                }
                Current::Opening(opening) => (
                    None,
                    ChannelState::Opening {
                        retrying: lock(&opening.retrying).clone(),
                    },
                ),
                Current::Stopped(failure) => (None, ChannelState::Stopped { failure }),
                Current::Duplicate { key, owner } => (Some(key), ChannelState::Duplicate { owner }),
            };
            statuses.push(ChannelStatus {
                name,
                key,
                state,
                menu_failure: None,
                recent_failures: Vec::new(),
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

/// The supervisor task's state: the slots it shares for status, and the identifications it runs.
struct Supervision {
    kernel: Arc<Kernel>,
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    current: Arc<Mutex<BTreeMap<String, Slot>>>,
    stop: CancellationToken,
    openings: JoinSet<Opened>,
}

impl Supervision {
    /// Reconcile with each view and admit identified accounts until stopped or the views end.
    async fn run(&mut self, mut views: watch::Receiver<Arc<View>>) -> Result<(), ChannelError> {
        loop {
            let view = views.borrow_and_update().clone();
            self.reconcile(&view).await?;
            loop {
                tokio::select! {
                    biased;
                    () = self.stop.cancelled() => return Ok(()),
                    changed = views.changed() => match changed {
                        Ok(()) => break,
                        Err(_) => return Ok(()),
                    },
                    Some(opened) = self.openings.join_next() => {
                        self.admit(opened.map_err(ChannelError::Unjoined)?);
                    }
                }
            }
        }
    }

    async fn reconcile(&mut self, view: &View) -> Result<(), ChannelError> {
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
        let old = lock(&self.current).clone();
        for (name, slot) in old {
            if settings.get(&name) != Some(&slot.settings) || routes.get(&name) != Some(&slot.route)
            {
                stop_slot(&slot).await?;
                lock(&self.current).remove(&name);
            }
        }
        {
            let mut current = lock(&self.current);
            let owned: BTreeSet<_> = current
                .values()
                .filter_map(|slot| match &slot.current {
                    Current::Running(channel) => Some(channel.key.clone()),
                    _ => None,
                })
                .collect();
            // A duplicate whose owner is gone opens again.
            current.retain(|_, slot| {
                !matches!(&slot.current, Current::Duplicate { key, .. } if !owned.contains(key))
            });
        }
        for (name, settings) in settings {
            if self.stop.is_cancelled() {
                break;
            }
            if lock(&self.current).contains_key(&name) {
                continue;
            }
            let route = routes[&name].clone();
            let current = match view.exports.channel(&settings.plugin) {
                Ok(export) => Current::Opening(Opening::start(
                    &mut self.openings,
                    name.clone(),
                    self.kernel.clone(),
                    self.store.clone(),
                    settings.transport(),
                    export,
                    self.stop.child_token(),
                )),
                Err(error) => Current::Stopped(Some(ChannelFailure::from(&ChannelError::Config(
                    error.message,
                )))),
            };
            lock(&self.current).insert(
                name,
                Slot {
                    settings,
                    route,
                    current,
                },
            );
        }
        Ok(())
    }

    /// Start an identified account unless another connection owns it; an opening its slot no
    /// longer holds was replaced, and its account is dropped.
    fn admit(&mut self, (name, opening, result): Opened) {
        let settings = match lock(&self.current).get(&name) {
            Some(Slot {
                settings,
                current: Current::Opening(active),
                ..
            }) if Arc::ptr_eq(active, &opening) => settings.clone(),
            _ => return,
        };
        let next = match result {
            Ok(Some(adapter)) => {
                let key = adapter.identity().key();
                match self.owner(&key) {
                    Some(owner) => Current::Duplicate { key, owner },
                    None => Current::Running(Arc::new(Channel::start(
                        self.kernel.clone(),
                        adapter,
                        settings,
                        opening.generation,
                        self.clock.clone(),
                    ))),
                }
            }
            Ok(None) => Current::Stopped(None),
            Err(error) => Current::Stopped(Some(ChannelFailure::from(&error))),
        };
        if let Some(slot) = lock(&self.current).get_mut(&name) {
            slot.current = next;
        }
    }

    /// The running connection that owns `key`.
    fn owner(&self, key: &str) -> Option<String> {
        lock(&self.current)
            .iter()
            .find_map(|(name, slot)| match &slot.current {
                Current::Running(channel) if channel.key == key => Some(name.clone()),
                _ => None,
            })
    }

    /// Stop every connection, then wait for every identification to end.
    async fn shutdown(&mut self) -> Result<(), ChannelError> {
        let snapshot = lock(&self.current).clone();
        let mut failure = None;
        for (name, slot) in snapshot {
            if let Err(error) = stop_slot(&slot).await {
                failure.get_or_insert(error);
            }
            if let Some(slot) = lock(&self.current).get_mut(&name) {
                slot.current = Current::Stopped(None);
            }
        }
        while let Some(opened) = self.openings.join_next().await {
            if let Err(error) = opened {
                failure.get_or_insert(ChannelError::Unjoined(error));
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

/// Stop a slot's connection and wait for it; an identification only reads, so it is signalled
/// and its end drained with the others.
async fn stop_slot(slot: &Slot) -> Result<(), ChannelError> {
    match &slot.current {
        Current::Opening(opening) => opening.stop(),
        Current::Running(channel) => {
            if let Err(error) = channel.shutdown().await {
                if matches!(error, ChannelError::Unjoined(_)) {
                    return Err(error);
                }
                tracing::error!(%error, "previous channel owner stopped with an error");
            }
        }
        Current::Stopped(_) | Current::Duplicate { .. } => {}
    }
    Ok(())
}
