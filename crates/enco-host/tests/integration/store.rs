#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
use crate::support;
use chrono::SubsecRound;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::{Accepted, Commit, ConnectionWrite, NewSession, Store, StoreError};
use std::path::Path;
mod plans;

/// A store holding the `main` Session, with a user message for it that is not yet accepted.
async fn main_session(root: &Path) -> (SqliteStore, Event) {
    let store = SqliteStore::open(support::store_paths(root)).await.unwrap();
    let session = store
        .ensure_session("main", Utc::now().trunc_subsecs(3))
        .await
        .unwrap();
    let event = Event {
        id: EventId::new(),
        session: session.id,
        source: EventSource::Cli,
        body: EventBody::UserMessage {
            quote: None,
            images: Vec::new(),
            unviewable: Vec::new(),
            earlier: Vec::new(),
            text: "hello".into(),
        },
        received_at: Utc::now().trunc_subsecs(3),
    };
    (store, event)
}

/// The first Log entry of the Session, consuming `event`.
fn consumed(event: &Event) -> Entry {
    Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(1),
        },
        at: Utc::now().trunc_subsecs(3),
        body: EntryBody::EventConsumed {
            event: event.clone(),
        },
    }
}

#[tokio::test]
async fn accepting_an_event_again_reports_a_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    assert_eq!(
        store
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap()
            .events,
        vec![Accepted::New]
    );
    assert_eq!(
        store
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap()
            .events,
        vec![Accepted::Duplicate]
    );
}

#[tokio::test]
async fn a_failed_acceptance_discards_its_connection_write() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    store
        .accept(std::slice::from_ref(&event), None)
        .await
        .unwrap();
    let write = ConnectionWrite {
        key: "channel:account".into(),
        state: serde_json::json!({"offset": 1}),
        settlement: None,
        update: None,
        session: None,
    };
    store.accept(&[], Some(&write)).await.unwrap();
    let uncommitted = Event {
        id: EventId::new(),
        ..event.clone()
    };
    let invalid = Event {
        id: EventId::new(),
        session: SessionId::new(),
        ..event.clone()
    };
    let advance = ConnectionWrite {
        state: serde_json::json!({"offset": 2}),
        update: Some("update-2".into()),
        session: Some(NewSession {
            id: SessionId::new(),
            name: "group".into(),
            profile: "default".into(),
            created_at: Utc::now().trunc_subsecs(3),
        }),
        ..write.clone()
    };
    assert!(
        store
            .accept(&[uncommitted, invalid], Some(&advance))
            .await
            .is_err()
    );
    assert_eq!(
        store.connection(&write.key).await.unwrap(),
        Some(write.state)
    );
    assert_eq!(store.pending(event.session).await.unwrap(), vec![event]);
    assert!(
        !store
            .connection_update_seen(&write.key, "update-2")
            .await
            .unwrap()
    );
    assert!(store.session_by_name("group").await.unwrap().is_none());
}

#[tokio::test]
async fn accepted_update_keys_survive_restarts_and_are_never_accepted_twice() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    let write = ConnectionWrite {
        key: "protocol:account".into(),
        state: serde_json::json!({"everyone": true}),
        update: Some("command-1".into()),
        session: None,
        settlement: None,
    };
    store.accept(&[], Some(&write)).await.unwrap();
    drop(store);
    let store = SqliteStore::open(support::store_paths(dir.path()))
        .await
        .unwrap();
    assert!(
        store
            .connection_update_seen(&write.key, "command-1")
            .await
            .unwrap()
    );
    let repeated = ConnectionWrite {
        state: serde_json::json!({"everyone": false}),
        ..write.clone()
    };
    assert!(
        store
            .accept(std::slice::from_ref(&event), Some(&repeated))
            .await
            .is_err()
    );
    assert!(store.pending(event.session).await.unwrap().is_empty());
    assert_eq!(
        store.connection(&write.key).await.unwrap(),
        Some(write.state)
    );
}

#[tokio::test]
async fn commits_must_continue_the_log_within_the_current_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    store
        .accept(std::slice::from_ref(&event), None)
        .await
        .unwrap();
    let entry = consumed(&event);
    let mut bad = entry.clone();
    bad.pos.seq = Seq(2);
    assert!(matches!(
        store
            .commit(
                event.session,
                Commit {
                    entries: vec![bad],
                    plan: None
                }
            )
            .await,
        Err(StoreError::OutOfOrder { .. })
    ));
    let mut bad = entry;
    bad.pos.epoch = Epoch(2);
    assert!(matches!(
        store
            .commit(
                event.session,
                Commit {
                    entries: vec![bad],
                    plan: None
                }
            )
            .await,
        Err(StoreError::Fenced { .. })
    ));
}

#[tokio::test]
async fn a_commit_consuming_an_unaccepted_event_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    store
        .accept(std::slice::from_ref(&event), None)
        .await
        .unwrap();
    let entry = consumed(&event);
    let missing = Event {
        id: EventId::new(),
        ..event.clone()
    };
    let second = Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(2),
        },
        body: EntryBody::EventConsumed {
            event: missing.clone(),
        },
        ..entry.clone()
    };
    // Consuming the first Event must roll back when the second Event was never accepted.
    assert!(
        store
            .commit(
                event.session,
                Commit {
                    entries: vec![entry, second],
                    plan: None,
                }
            )
            .await
            .is_err()
    );
    assert!(store.log(event.session, None).await.unwrap().is_empty());
    assert_eq!(store.pending(event.session).await.unwrap(), vec![event]);
}

#[tokio::test]
async fn committed_entries_consume_their_events_and_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    store
        .accept(std::slice::from_ref(&event), None)
        .await
        .unwrap();
    let entry = consumed(&event);
    store
        .commit(
            event.session,
            Commit {
                entries: vec![entry.clone()],
                plan: None,
            },
        )
        .await
        .unwrap();
    assert!(store.pending(event.session).await.unwrap().is_empty());
    assert_eq!(
        store.log(event.session, None).await.unwrap(),
        vec![entry.clone()]
    );
    let node = store.node().await.unwrap().id;
    drop(store);
    let reopened = SqliteStore::open(support::store_paths(dir.path()))
        .await
        .unwrap();
    assert_eq!(reopened.node().await.unwrap().id, node);
    assert_eq!(
        reopened.log(event.session, None).await.unwrap(),
        vec![entry]
    );
    assert!(reopened.pending(event.session).await.unwrap().is_empty());
    assert_eq!(
        reopened
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap()
            .events,
        vec![Accepted::Duplicate]
    );
}

#[tokio::test]
async fn blob_reads_reject_content_that_no_longer_matches_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(support::store_paths(dir.path()))
        .await
        .unwrap();
    let hash = store.put_blob(b"original").await.unwrap();
    assert_eq!(store.get_blob(&hash).await.unwrap(), b"original");
    std::fs::write(store.blob_path(&hash), b"changed").unwrap();
    assert!(matches!(
        store.get_blob(&hash).await,
        Err(StoreError::Blob(_))
    ));
}

#[tokio::test]
async fn schedule_cursor_and_inbox_delivery_commit_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(support::store_paths(dir.path()))
        .await
        .unwrap();
    let now: DateTime<Utc> = "2026-10-09T00:00:00Z".parse().unwrap();
    let session = store.ensure_session("main", now).await.unwrap();
    let schedule = Schedule {
        id: ScheduleId::new(),
        session: session.id,
        rule: ScheduleRule::Cron {
            expr: "0 8 * * *".into(),
            timezone: None,
        },
        message: "briefing".into(),
        created_at: now,
        state: ScheduleState::Active,
        last: None,
    };
    store.insert_schedule(&schedule).await.unwrap();
    let mut event = Event {
        id: EventId::new(),
        session: session.id,
        source: EventSource::Scheduler,
        body: EventBody::Reminder {
            schedule: schedule.id,
            due_at: "2026-10-09T08:00:00Z".parse().unwrap(),
            skipped: 0,
            text: schedule.message.clone(),
        },
        received_at: now,
    };
    store
        .accept(std::slice::from_ref(&event), None)
        .await
        .unwrap();
    // A duplicate Inbox ID fails after the cursor update; neither write may commit alone.
    assert!(store.fire_schedule(None, false, &event).await.is_err());
    assert_eq!(store.schedules(None).await.unwrap(), vec![schedule.clone()]);
    store
        .commit(
            session.id,
            Commit {
                entries: vec![consumed(&event)],
                plan: None,
            },
        )
        .await
        .unwrap();
    assert!(store.fire_schedule(None, false, &event).await.is_err());
    assert_eq!(store.schedules(None).await.unwrap(), vec![schedule.clone()]);
    event.id = EventId::new();
    store.fire_schedule(None, false, &event).await.unwrap();
    let last = store.schedules(None).await.unwrap()[0].last.unwrap();
    assert_eq!(last.event, event.id);
    let stale = Event {
        id: EventId::new(),
        ..event.clone()
    };
    assert!(matches!(
        store.fire_schedule(None, false, &stale).await,
        Err(StoreError::ScheduleNotActive(_))
    ));
    assert_eq!(store.pending(session.id).await.unwrap().len(), 1);
}
