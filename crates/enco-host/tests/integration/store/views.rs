//! A Session row is the fold of its creation record, its archive and its own Log, and its schedule
//! rows the fold of its own Log, after every commit that records one of those facts; the
//! effective views continue those folds over the pending Inbox.
use super::*;
use enco_kernel::SessionDeliveryKind;
use rusqlite::OptionalExtension;

fn owner(session: SessionId, body: EventBody) -> Event {
    Event {
        id: EventId::new(),
        session,
        source: EventSource::Cli,
        body,
        received_at: Utc::now().trunc_subsecs(3),
    }
}

fn reminder(session: SessionId, schedule: ScheduleId) -> Event {
    let due_at = Utc::now().trunc_subsecs(3);
    Event {
        id: EventId::reminder(schedule, due_at),
        session,
        source: EventSource::Scheduler,
        body: EventBody::Reminder {
            schedule,
            due_at,
            skipped: 0,
            text: "briefing".into(),
        },
        received_at: due_at,
    }
}

fn hourly() -> ScheduleRule {
    ScheduleRule::Cron {
        expr: "0 * * * *".into(),
        timezone: None,
    }
}

fn directory(store_node: NodeId, path: &str) -> EventBody {
    EventBody::SetWorkingDir {
        working_dir: WorkingDir::new(store_node, path.into()).unwrap(),
    }
}

/// The facts of one Session, read where their authorities keep them.
struct Facts {
    db: rusqlite::Connection,
}

impl Facts {
    fn creation(&self, log_of_parent: &[Entry], id: SessionId) -> Creation {
        let root = self
            .db
            .query_row(
                "SELECT name,created_at,node,profile,working_dir_node,working_dir_path
                 FROM root_directory WHERE id=?",
                [id.to_string()],
                |row| {
                    let text = |column| row.get::<_, String>(column);
                    Ok((text(0)?, text(1)?, text(2)?, text(3)?, text(4)?, text(5)?))
                },
            )
            .optional()
            .unwrap();
        match root {
            Some((name, created_at, node, profile, dir_node, dir_path)) => Creation {
                id,
                name: name.parse().unwrap(),
                parent: None,
                created_at: created_at.parse().unwrap(),
                node: node.parse().unwrap(),
                profile: profile.parse().unwrap(),
                working_dir: WorkingDir::new(dir_node.parse().unwrap(), dir_path.into()).unwrap(),
            },
            // A child's record is in its parent's Log, with the settlement that created it.
            None => log_of_parent
                .iter()
                .find_map(|entry| match &entry.body {
                    EntryBody::ToolCallSettled {
                        delegated:
                            Some(Delegated {
                                creation: Some(creation),
                                ..
                            }),
                        ..
                    } if creation.id == id => Some(creation.clone()),
                    _ => None,
                })
                .unwrap(),
        }
    }

    /// The archive derived down the tree: its own record, else its nearest recorded ancestor's.
    fn archived_at(&self, row: &SessionRecord, parent: Option<&SessionRecord>) -> Option<String> {
        self.db
            .query_row(
                "SELECT at FROM archives WHERE session_id=?",
                [row.id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .unwrap()
            .or_else(|| parent.and_then(|parent| self.archived_at(parent, None)))
    }
}

/// The views must equal the folds of the facts recorded so far.
async fn check(store: &SqliteStore, facts: &Facts, id: SessionId, parent: Option<&SessionRecord>) {
    let parent_log = match parent {
        Some(parent) => store.log(parent.id, None).await.unwrap(),
        None => Vec::new(),
    };
    let row = store.committed_session(id).await.unwrap().unwrap();
    let log = store.log(id, None).await.unwrap();
    let archived_at = facts
        .archived_at(&row, parent)
        .map(|at| at.parse().unwrap());
    let fold = SessionRecord::fold(
        &facts.creation(&parent_log, id),
        archived_at,
        log.iter().map(|entry| &entry.body),
    );
    assert_eq!(row, fold);
    let mut schedules = ScheduleFold::new(id);
    for entry in &log {
        schedules.apply(&entry.body);
    }
    // The effective views continue the same folds over the pending Inbox.
    let mut effective = fold;
    for event in store.pending(id).await.unwrap() {
        effective.consume(&event.body);
        schedules.consume(&event);
    }
    // An archived Session has no schedules to list.
    let listed = match effective.archived_at {
        None => schedules.rows(),
        Some(_) => Vec::new(),
    };
    assert_eq!(store.session(id).await.unwrap().unwrap(), effective);
    assert_eq!(store.schedules(Some(id)).await.unwrap(), listed);
}

#[tokio::test]
async fn every_commit_leaves_rows_equal_to_the_fold_of_their_facts() {
    let dir = tempfile::tempdir().unwrap();
    let (store, message) = main_session(dir.path()).await;
    let facts = Facts {
        db: rusqlite::Connection::open(dir.path().join("enco.db")).unwrap(),
    };
    let node = store.node().await.unwrap().id;
    let main = Actor {
        store: &store,
        session: message.session,
    };
    check(&store, &facts, main.session, None).await;
    main.consume(EventBody::SetProfile {
        profile: "research".parse().unwrap(),
    })
    .await;
    check(&store, &facts, main.session, None).await;
    let briefing = main.create_schedule(hourly()).await;
    let digest = main.create_schedule(hourly()).await;
    check(&store, &facts, main.session, None).await;
    // Pending commands and reminders show in the effective views only.
    let pending = [
        owner(main.session, directory(node, "/pending")),
        owner(
            main.session,
            EventBody::CancelSchedule { schedule: briefing },
        ),
        reminder(main.session, digest),
    ];
    store.accept(&pending, None).await.unwrap();
    check(&store, &facts, main.session, None).await;
    main.append(
        pending
            .into_iter()
            .map(|event| EntryBody::EventConsumed { event })
            .collect(),
        Vec::new(),
    )
    .await;
    check(&store, &facts, main.session, None).await;
    main.consume(message.body.clone()).await;
    main.settle_schedule(ScheduleChange::Cancelled { id: digest })
        .await;
    check(&store, &facts, main.session, None).await;
    // A backtrack leaves the facts outside the conversation as they are.
    main.consume(EventBody::Backtrack { to: None }).await;
    check(&store, &facts, main.session, None).await;
    let parent = store
        .committed_session(main.session)
        .await
        .unwrap()
        .unwrap();
    let child = delegate(&main, &parent).await;
    check(&store, &facts, main.session, None).await;
    check(&store, &facts, child, Some(&parent)).await;
    let child_actor = Actor {
        store: &store,
        session: child,
    };
    child_actor.consume(directory(node, "/child")).await;
    check(&store, &facts, child, Some(&parent)).await;
    // Archiving the root is one fact; the child is archived because its parent is.
    store
        .halt_subtree(
            main.session,
            EventSource::Cli,
            Halt {
                root: SessionRef {
                    id: parent.id,
                    name: parent.name.clone(),
                },
                by: None,
            },
            Utc::now().trunc_subsecs(3),
            true,
        )
        .await
        .unwrap();
    let archived = store
        .committed_session(main.session)
        .await
        .unwrap()
        .unwrap();
    check(&store, &facts, main.session, None).await;
    check(&store, &facts, child, Some(&archived)).await;
}

/// Delegate to a new `research` child, its creation record in the settlement that delivers to it.
async fn delegate(main: &Actor<'_>, parent: &SessionRecord) -> SessionId {
    let creation = Creation {
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
    };
    let task = Event {
        id: EventId::new(),
        session: creation.id,
        source: EventSource::Parent,
        body: EventBody::SessionMessage(Outgoing {
            text: "task".into(),
            file: None,
        }),
        received_at: Utc::now().trunc_subsecs(3),
    };
    let settled = EntryBody::ToolCallSettled {
        call: CallId::new(),
        outcome: Settlement::Ok,
        content: "Created child Session `research` and sent it the message.".into(),
        full: None,
        images: Vec::new(),
        outgoing: None,
        delegated: Some(Delegated {
            session: SessionRef {
                id: creation.id,
                name: creation.name.clone(),
            },
            creation: Some(creation.clone()),
            event: task.id,
        }),
        schedule: None,
    };
    main.append(
        vec![settled],
        vec![SessionDelivery {
            event: task,
            kind: SessionDeliveryKind::Delegation {
                name: creation.name.clone(),
                create: Some(Box::new(creation.clone())),
                profile: None,
                working_dir: None,
            },
        }],
    )
    .await;
    creation.id
}
