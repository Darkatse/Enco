//! Atomic delegation rejection and archive behavior through the persistence port.
use super::*;
use enco_kernel::{CommitRejection, SessionDeliveryKind};

fn outgoing(session: SessionId) -> Event {
    Event {
        id: EventId::new(),
        session,
        source: EventSource::Parent,
        body: EventBody::SessionMessage(Outgoing {
            text: "task".into(),
            file: None,
        }),
        received_at: Utc::now().trunc_subsecs(3),
    }
}
/// The owner's halt of the subtree under `root`.
fn owner_halt(root: &SessionRecord) -> Halt {
    Halt {
        root: SessionRef {
            id: root.id,
            name: root.name.clone(),
        },
        by: None,
    }
}

/// The record a parent writes when it creates its `research` child.
fn creation(parent: &SessionRecord) -> Creation {
    Creation {
        id: SessionId::new(),
        name: parent.name.child("research").unwrap(),
        parent: Some(SessionRef {
            id: parent.id,
            name: parent.name.clone(),
        }),
        created_at: Utc::now().trunc_subsecs(3),
        node: parent.binding.node,
        profile: parent.profile.clone(),
        working_dir: parent.working_dir.clone(),
    }
}
async fn append(
    store: &SqliteStore,
    sender: SessionId,
    input: &Event,
    delivery: SessionDelivery,
) -> CommitOutcome {
    store
        .accept(std::slice::from_ref(input), None)
        .await
        .unwrap();
    let seq = store
        .log(sender, None)
        .await
        .unwrap()
        .last()
        .map_or(1, |entry| entry.pos.seq.0 + 1);
    let mut entry = consumed(input);
    entry.pos.seq = Seq(seq);
    store
        .commit(
            sender,
            Commit {
                entries: vec![entry],
                plan: None,
                deliveries: vec![delivery],
            },
        )
        .await
        .unwrap()
}
async fn create_child(store: &SqliteStore, parent: &SessionRecord, input: &Event) -> SessionRecord {
    let child = creation(parent);
    let id = child.id;
    let name = child.name.clone();
    assert!(matches!(append(store, parent.id, input, SessionDelivery {
        event: outgoing(id), kind: SessionDeliveryKind::Delegation { name, create: Some(Box::new(child)), profile: None, working_dir: None },
    }).await, CommitOutcome::Committed { delivered } if delivered == [id]));
    store.session(id).await.unwrap().unwrap()
}

#[tokio::test]
async fn child_creation_and_profile_assertions_are_atomic_with_the_senders_facts() {
    let dir = tempfile::tempdir().unwrap();
    let (store, input) = main_session(dir.path()).await;
    let parent = store.session(input.session).await.unwrap().unwrap();
    let child = create_child(&store, &parent, &input).await;
    assert_eq!(
        child.parent,
        Some(SessionRef {
            id: parent.id,
            name: parent.name.clone()
        })
    );
    let retry = Event {
        id: EventId::new(),
        ..input.clone()
    };
    let rejected = append(
        &store,
        parent.id,
        &retry,
        SessionDelivery {
            event: outgoing(child.id),
            kind: SessionDeliveryKind::Delegation {
                name: child.name.clone(),
                create: None,
                profile: Some("other".parse().unwrap()),
                working_dir: None,
            },
        },
    )
    .await;
    assert!(matches!(
        rejected,
        CommitOutcome::Rejected(CommitRejection::ProfileMismatch { .. })
    ));
    assert_eq!(
        store.pending(parent.id).await.unwrap(),
        std::slice::from_ref(&retry)
    );
    assert_eq!(store.log(parent.id, None).await.unwrap().len(), 1);
    assert_eq!(store.pending(child.id).await.unwrap().len(), 1);
    let other_dir = WorkingDir::new(parent.working_dir.node, dir.path().join("other")).unwrap();
    let rejected = append(
        &store,
        parent.id,
        &retry,
        SessionDelivery {
            event: outgoing(child.id),
            kind: SessionDeliveryKind::Delegation {
                name: child.name.clone(),
                create: None,
                profile: None,
                working_dir: Some(other_dir),
            },
        },
    )
    .await;
    assert!(matches!(
        rejected,
        CommitOutcome::Rejected(CommitRejection::WorkingDirMismatch { .. })
    ));
    let duplicate_child = creation(&parent);
    let rejected = append(
        &store,
        parent.id,
        &retry,
        SessionDelivery {
            event: outgoing(duplicate_child.id),
            kind: SessionDeliveryKind::Delegation {
                name: duplicate_child.name.clone(),
                create: Some(Box::new(duplicate_child)),
                profile: None,
                working_dir: None,
            },
        },
    )
    .await;
    assert!(matches!(
        rejected,
        CommitOutcome::Rejected(CommitRejection::ChildExists(_))
    ));
    assert_eq!(store.sessions().await.unwrap().len(), 2);
}

#[tokio::test]
async fn pending_halts_reject_tools_but_keep_boundary_facts_without_delivering() {
    let dir = tempfile::tempdir().unwrap();
    let (store, input) = main_session(dir.path()).await;
    let parent = store.session(input.session).await.unwrap().unwrap();
    let child = create_child(&store, &parent, &input).await;
    store
        .halt_subtree(
            child.id,
            EventSource::Cli,
            owner_halt(&child),
            Utc::now(),
            false,
        )
        .await
        .unwrap();
    let pending = store.pending(child.id).await.unwrap();
    let first = pending[0].clone();
    // Consumption before the delivery check must also roll back on tool rejection.
    let rejected = append(
        &store,
        child.id,
        &first,
        SessionDelivery {
            event: outgoing(parent.id),
            kind: SessionDeliveryKind::Message,
        },
    )
    .await;
    assert!(matches!(
        rejected,
        CommitOutcome::Rejected(CommitRejection::Halted)
    ));
    assert_eq!(store.pending(child.id).await.unwrap(), pending);
    assert!(store.log(child.id, None).await.unwrap().is_empty());
    let new = creation(&child);
    let rejected = append(
        &store,
        child.id,
        &first,
        SessionDelivery {
            event: outgoing(new.id),
            kind: SessionDeliveryKind::Delegation {
                name: new.name.clone(),
                create: Some(Box::new(new)),
                profile: None,
                working_dir: None,
            },
        },
    )
    .await;
    assert!(matches!(
        rejected,
        CommitOutcome::Rejected(CommitRejection::Halted)
    ));
    let committed = append(
        &store,
        child.id,
        &first,
        SessionDelivery {
            event: outgoing(parent.id),
            kind: SessionDeliveryKind::Boundary,
        },
    )
    .await;
    assert!(matches!(committed, CommitOutcome::Committed { delivered } if delivered.is_empty()));
    assert!(store.pending(parent.id).await.unwrap().is_empty());
    assert_eq!(store.log(child.id, None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn archive_atomically_halts_the_active_subtree_and_releases_names() {
    let dir = tempfile::tempdir().unwrap();
    let (store, input) = main_session(dir.path()).await;
    let parent = store.session(input.session).await.unwrap().unwrap();
    let child = create_child(&store, &parent, &input).await;
    let at = Utc::now().trunc_subsecs(3);
    let halt = owner_halt(&parent);
    let ids: Vec<_> = store
        .halt_subtree(parent.id, EventSource::Cli, halt.clone(), at, true)
        .await
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .collect();
    assert!(ids.contains(&parent.id) && ids.contains(&child.id));
    for id in ids {
        let record = store.session(id).await.unwrap().unwrap();
        assert_eq!(record.archived_at, Some(at));
        let pending = store.pending(id).await.unwrap();
        let event = pending.last().unwrap();
        assert_eq!(event.body, EventBody::Halt(halt.clone()));
        assert_eq!(
            event.source,
            if id == parent.id {
                EventSource::Cli
            } else {
                EventSource::Parent
            }
        );
        assert_eq!(
            store.accept(&[outgoing(id)], None).await.unwrap().events,
            [Accepted::Archived]
        );
        assert_eq!(store.pending(id).await.unwrap(), pending);
    }
    assert!(store.session_by_name(&parent.name).await.unwrap().is_none());
    // One control-plane fact archives the subtree; the child is archived because its parent is.
    let db = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
    let archives: Vec<String> = db
        .prepare("SELECT session_id FROM archives")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(archives, [parent.id.to_string()]);
    let replacement = store.ensure_session(&parent.name, at).await.unwrap();
    assert_ne!(replacement.id, parent.id);
    assert_eq!(replacement.parent, None);
    // The archived child's name is free for the new root's child at once.
    let task = Event {
        session: replacement.id,
        id: EventId::new(),
        ..input.clone()
    };
    let new_child = create_child(&store, &replacement, &task).await;
    assert_eq!(new_child.name, child.name);
    assert_ne!(new_child.id, child.id);
    assert!(
        store
            .halt_subtree(parent.id, EventSource::Cli, halt, at, true)
            .await
            .unwrap()
            .is_empty()
    );
    drop(store);
    let store = support::open_store(dir.path()).await;
    assert_eq!(
        store
            .session_by_name(&parent.name)
            .await
            .unwrap()
            .unwrap()
            .id,
        replacement.id
    );
    assert_eq!(
        store
            .session(child.id)
            .await
            .unwrap()
            .unwrap()
            .parent
            .unwrap()
            .id,
        parent.id
    );
}

#[tokio::test]
async fn archived_sessions_list_no_schedules_and_take_no_reminders() {
    let dir = tempfile::tempdir().unwrap();
    let (store, input) = main_session(dir.path()).await;
    let session = store.session(input.session).await.unwrap().unwrap();
    let main = Actor {
        store: &store,
        session: session.id,
    };
    let at = Utc::now().trunc_subsecs(3);
    let schedule = main
        .create_schedule(ScheduleRule::Once {
            at: at + chrono::Duration::hours(1),
        })
        .await;
    let recorded = store.log(session.id, None).await.unwrap();
    store
        .halt_subtree(session.id, EventSource::Cli, owner_halt(&session), at, true)
        .await
        .unwrap();
    assert!(store.schedules(None).await.unwrap().is_empty());
    let reminder = Event {
        id: EventId::reminder(schedule, at + chrono::Duration::hours(1)),
        body: EventBody::Reminder {
            schedule,
            due_at: at + chrono::Duration::hours(1),
            skipped: 0,
            text: "briefing".into(),
        },
        source: EventSource::Scheduler,
        ..input
    };
    assert_eq!(
        store
            .accept(std::slice::from_ref(&reminder), None)
            .await
            .unwrap()
            .events,
        [Accepted::Archived]
    );
    // The Session's own facts are as they were.
    assert_eq!(store.log(session.id, None).await.unwrap(), recorded);
}
