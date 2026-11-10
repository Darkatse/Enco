#![expect(
    clippy::unwrap_used,
    reason = "helpers outside #[test] functions fail the test by panicking"
)]
use crate::support;
use chrono::SubsecRound;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::{
    Accepted, Commit, CommitOutcome, ConnectionWrite, NewSession, SessionDelivery, Store,
    StoreError,
};
use std::path::Path;
mod delegation;
mod migration;
mod plans;
mod views;

/// Writes facts the way a Session actor does: consumes accepted inputs, appends at the next
/// position, and hands deliveries to other Inboxes in the same transaction.
pub(crate) struct Actor<'a> {
    pub store: &'a SqliteStore,
    pub session: SessionId,
}

impl Actor<'_> {
    pub async fn try_append(
        &self,
        bodies: Vec<EntryBody>,
        deliveries: Vec<SessionDelivery>,
    ) -> Result<CommitOutcome, StoreError> {
        let next = self
            .store
            .log(self.session, None)
            .await
            .unwrap()
            .last()
            .map_or(1, |entry| entry.pos.seq.0 + 1);
        let entries = bodies
            .into_iter()
            .zip(next..)
            .map(|(body, seq)| Entry {
                pos: LogPos {
                    epoch: Epoch(1),
                    seq: Seq(seq),
                },
                at: Utc::now().trunc_subsecs(3),
                body,
            })
            .collect();
        let commit = Commit {
            entries,
            plan: None,
            deliveries,
        };
        self.store.commit(self.session, commit).await
    }

    pub async fn append(&self, bodies: Vec<EntryBody>, deliveries: Vec<SessionDelivery>) {
        let outcome = self.try_append(bodies, deliveries).await.unwrap();
        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
    }

    /// Accept an input from the owner, then consume it.
    pub async fn consume(&self, body: EventBody) {
        let event = Event {
            id: EventId::new(),
            session: self.session,
            source: EventSource::Cli,
            body,
            received_at: Utc::now().trunc_subsecs(3),
        };
        self.store
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap();
        self.append(vec![EntryBody::EventConsumed { event }], Vec::new())
            .await;
    }

    /// Settle a call that changed this Session's schedules.
    pub async fn settle_schedule(&self, change: ScheduleChange) {
        let body = EntryBody::ToolCallSettled {
            call: CallId::new(),
            outcome: Settlement::Ok,
            content: String::new(),
            full: None,
            images: Vec::new(),
            outgoing: None,
            delegated: None,
            schedule: Some(change),
        };
        self.append(vec![body], Vec::new()).await;
    }

    /// Settle a call that created a schedule with `rule`.
    pub async fn create_schedule(&self, rule: ScheduleRule) -> ScheduleId {
        let id = ScheduleId::new();
        self.settle_schedule(ScheduleChange::Created {
            id,
            rule,
            message: "briefing".into(),
            created_at: Utc::now().trunc_subsecs(3),
        })
        .await;
        id
    }
}

/// A store holding the `main` Session, with a user message for it that is not yet accepted.
async fn main_session(root: &Path) -> (SqliteStore, Event) {
    let store = support::open_store(root).await;
    let session = store
        .ensure_session(&"main".parse().unwrap(), Utc::now().trunc_subsecs(3))
        .await
        .unwrap();
    let event = Event {
        id: EventId::new(),
        session: session.id,
        source: EventSource::Cli,
        body: EventBody::UserMessage {
            quote: None,
            attachments: Vec::new(),
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
            name: "group".parse().unwrap(),
            profile: "default".parse().unwrap(),
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
    let store = support::open_store(dir.path()).await;
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
                    deliveries: Vec::new(),
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
                    deliveries: Vec::new(),
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
                    deliveries: Vec::new(),
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
                deliveries: Vec::new(),
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
    let reopened = support::open_store(dir.path()).await;
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
    let store = support::open_store(dir.path()).await;
    let hash = store.put_blob(b"original").await.unwrap();
    assert_eq!(store.get_blob(&hash).await.unwrap(), b"original");
    std::fs::write(store.blob_path(&hash), b"changed").unwrap();
    assert!(matches!(
        store.get_blob(&hash).await,
        Err(StoreError::Blob(_))
    ));
}

#[tokio::test]
async fn evidence_is_kept_once_and_read_back_only_when_it_matches_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let store = support::open_store(dir.path()).await;
    let record =
        br#"{"code":{"kind":"factory","plugin":"01J0000000000000000000TEST","artifact":"00"},"export":"completion.decode","arguments":{}}"#
            .repeat(20);
    let hash = store.put_evidence(&record).await.unwrap();
    assert_eq!(store.put_evidence(&record).await.unwrap(), hash);
    assert_eq!(store.evidence(&hash).await.unwrap(), record);
    let db = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
    let stored: Vec<u8> = db
        .query_row("SELECT body FROM evidence", [], |row| row.get(0))
        .unwrap();
    assert!(stored.len() < record.len());
    let changed = zstd::stream::encode_all(&b"changed"[..], 3).unwrap();
    db.execute("UPDATE evidence SET body=?", [changed]).unwrap();
    assert!(matches!(
        store.evidence(&hash).await,
        Err(StoreError::Evidence(_))
    ));
    assert!(matches!(
        store.evidence(&ContentHash::of(b"never stored")).await,
        Err(StoreError::Evidence(_))
    ));
}

/// Accept the reminder of `schedule` planned for `hour` o'clock on 10 November 2026.
async fn remind(
    store: &SqliteStore,
    session: SessionId,
    schedule: ScheduleId,
    hour: u32,
) -> Accepted {
    let due_at = "2026-11-10T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
        + chrono::Duration::hours(i64::from(hour));
    let event = Event {
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
    };
    store
        .accept(std::slice::from_ref(&event), None)
        .await
        .unwrap()
        .events[0]
}

#[tokio::test]
async fn a_reminder_is_written_once_and_only_while_its_schedule_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let (store, input) = main_session(dir.path()).await;
    let main = Actor {
        store: &store,
        session: input.session,
    };
    let briefing = main
        .create_schedule(ScheduleRule::Cron {
            expr: "0 * * * *".into(),
            timezone: None,
        })
        .await;
    assert_eq!(
        remind(&store, main.session, briefing, 1).await,
        Accepted::New
    );
    // The planned time decides the identity, so the same occurrence is a duplicate.
    assert_eq!(
        remind(&store, main.session, briefing, 1).await,
        Accepted::Duplicate
    );
    // Once the owner's cancel is accepted, no later occurrence is written, though the Session has
    // not consumed the cancel yet; the reminder accepted before it stays.
    let cancel = Event {
        id: EventId::new(),
        session: main.session,
        source: EventSource::Cli,
        body: EventBody::CancelSchedule { schedule: briefing },
        received_at: Utc::now().trunc_subsecs(3),
    };
    store
        .accept(std::slice::from_ref(&cancel), None)
        .await
        .unwrap();
    assert_eq!(
        remind(&store, main.session, briefing, 2).await,
        Accepted::Inactive
    );
    assert_eq!(store.pending(main.session).await.unwrap().len(), 2);
    // A one-time schedule has one occurrence.
    let once = main
        .create_schedule(ScheduleRule::Once {
            at: Utc::now().trunc_subsecs(3) + chrono::Duration::hours(1),
        })
        .await;
    assert_eq!(remind(&store, main.session, once, 1).await, Accepted::New);
    assert_eq!(
        remind(&store, main.session, once, 2).await,
        Accepted::Inactive
    );
    // A schedule this Session never created has no reminders, and no fact may name it.
    let unknown = ScheduleId::new();
    assert_eq!(
        remind(&store, main.session, unknown, 1).await,
        Accepted::Inactive
    );
    let before = store.log(main.session, None).await.unwrap();
    assert!(matches!(
        main.try_append(
            vec![EntryBody::ToolCallSettled {
                call: CallId::new(),
                outcome: Settlement::Ok,
                content: String::new(),
                full: None,
                images: Vec::new(),
                outgoing: None,
                delegated: None,
                schedule: Some(ScheduleChange::Cancelled { id: unknown }),
            }],
            Vec::new()
        )
        .await,
        Err(StoreError::UnknownSchedule { schedule, .. }) if schedule == unknown
    ));
    assert_eq!(store.log(main.session, None).await.unwrap(), before);
}
