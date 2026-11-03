use super::*;

/// Where the process is killed while a child Session runs its task.
#[derive(Clone, Copy, PartialEq)]
enum Killed {
    Running,
    AfterHalt,
    AfterArchive,
}

#[tokio::test]
async fn restart_reports_interrupted_children_and_finishes_committed_halts() {
    for killed in [Killed::Running, Killed::AfterHalt, Killed::AfterArchive] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let worker = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (h, parent, child, mut held) = worker
            .spawn(async move {
                let mut h = Harness::new(&root, |_| {}).await;
                let (parent, mut rx) = start(&h).await;
                h.next("fixture").await.respond(calls(vec![send(
                    "research",
                    "task",
                    Some("worker"),
                )]));
                let held = h.next("worker").await;
                h.next("fixture").await.respond(reply("delegated"));
                finish(&mut rx).await;
                let child = h.child("main/research").await;
                (h, parent, child, held)
            })
            .await
            .unwrap();
        if killed != Killed::Running {
            // Kill after the halt commit, before the actor receives a cancellation signal.
            h.store
                .halt_subtree(
                    child.id,
                    EventSource::Cli,
                    Halt {
                        root: SessionRef {
                            id: child.id,
                            name: child.name.clone(),
                        },
                        by: None,
                    },
                    Utc::now(),
                    killed == Killed::AfterArchive,
                )
                .await
                .unwrap();
        }
        worker.shutdown_background();
        tokio::time::timeout(Duration::from_secs(15), held.answer.closed())
            .await
            .unwrap();
        drop((held, h));
        let mut h = Harness::new(dir.path(), |_| {}).await;
        if killed == Killed::Running {
            let parent_prompt = h.next("fixture").await;
            assert!(!h.latest("main").children[0].state.awaiting);
            let mut rx = h.kernel.subscribe(parent).await.unwrap();
            parent_prompt.respond(crate::silence::skip("restart noted"));
            finish(&mut rx).await;
            let entries = h.kernel.log(parent, None).await.unwrap();
            assert!(entries.iter().any(|entry| matches!(
                &entry.body,
                EntryBody::EventConsumed {
                    event: Event {
                        body: EventBody::ChildRunEnded {
                            end: RunEnd::Interrupted
                        },
                        ..
                    }
                }
            )));
        } else {
            halted_after_restart(&h, parent, child.id, killed == Killed::AfterArchive).await;
        }
        let entries = h.kernel.log(child.id, None).await.unwrap();
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(
                    entry.body,
                    EntryBody::RunEnded {
                        end: RunEnd::Interrupted,
                        ..
                    }
                ))
                .count(),
            1
        );
        delegated_once(&h, &entries).await;
        h.kernel.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn profiles_used_only_by_archived_sessions_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new(dir.path(), |_| {}).await;
    let (_, mut rx) = start(&h).await;
    h.next("fixture")
        .await
        .respond(calls(vec![send("research", "task", Some("worker"))]));
    let held = h.next("worker").await;
    h.next("fixture").await.respond(reply("delegated"));
    finish(&mut rx).await;
    let child = h.child("main/research").await;
    let mut document = h.kernel.registry().view().config.document.clone();
    document["profile"]
        .as_object_mut()
        .unwrap()
        .remove("worker");
    assert!(
        submit(h.kernel.registry(), configuration_request(document.clone()))
            .await
            .is_err()
    );
    h.kernel.archive(child.id, EventSource::Cli).await.unwrap();
    submit(h.kernel.registry(), configuration_request(document))
        .await
        .unwrap();
    assert!(
        !h.kernel
            .registry()
            .view()
            .config
            .kernel
            .profiles
            .contains_key("worker")
    );
    drop(held);
    h.kernel.shutdown().await.unwrap();
}

/// The halt committed before the kill takes effect once after restart, and wakes no parent.
async fn halted_after_restart(h: &Harness, parent: SessionId, child: SessionId, archived: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let stopped = !archived
                || matches!(
                    h.kernel.subscribe(child).await.unwrap().try_recv(),
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed)
                );
            if h.store.pending(child).await.unwrap().is_empty() && stopped {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let halts = h
        .kernel
        .log(child, None)
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| {
            matches!(
                &entry.body,
                EntryBody::EventConsumed {
                    event: Event {
                        body: EventBody::Halt(_),
                        ..
                    }
                }
            )
        })
        .count();
    assert_eq!(halts, 1);
    // The child's recorded end would have been delivered in the same commit; the halt blocks it.
    let pending = h.store.pending(parent).await.unwrap();
    assert!(
        !pending
            .iter()
            .any(|event| matches!(event.source, EventSource::Child(_)))
    );
    assert!(!from_child(&h.kernel.log(parent, None).await.unwrap()));
    let record = h.store.session(child).await.unwrap().unwrap();
    assert_eq!(record.archived_at.is_some(), archived);
}

/// A delegation committed before the kill left exactly one child with exactly one task.
async fn delegated_once(h: &Harness, child: &[Entry]) {
    let named = h.kernel.sessions().await.unwrap();
    assert_eq!(
        named
            .iter()
            .filter(|session| session.name.as_str() == "main/research")
            .count(),
        1
    );
    assert_eq!(
        child
            .iter()
            .filter(|entry| matches!(
                &entry.body,
                EntryBody::EventConsumed {
                    event: Event {
                        source: EventSource::Parent,
                        body: EventBody::SessionMessage(_),
                        ..
                    }
                }
            ))
            .count(),
        1
    );
}
