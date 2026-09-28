use chrono::SubsecRound;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::{Accepted, Commit, Store, StoreError};

#[tokio::test]
async fn acceptance_and_log_commit_are_atomic_and_structurally_ordered() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("enco.db"), dir.path().join("blobs"))
        .await
        .unwrap();
    let session = store
        .ensure_session("main", Utc::now().trunc_subsecs(3))
        .await
        .unwrap();
    let event = Event {
        id: EventId::new(),
        session: session.id,
        source: EventSource::Cli,
        body: EventBody::UserMessage {
            text: "hello".into(),
        },
        received_at: Utc::now().trunc_subsecs(3),
    };
    assert_eq!(store.accept(&event).await.unwrap(), Accepted::New);
    assert_eq!(store.accept(&event).await.unwrap(), Accepted::Duplicate);
    let entry = Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(1),
        },
        at: Utc::now().trunc_subsecs(3),
        body: EntryBody::EventConsumed {
            event: event.clone(),
        },
    };
    let mut bad = entry.clone();
    bad.pos.seq = Seq(2);
    assert!(matches!(
        store
            .commit(
                session.id,
                Commit {
                    entries: vec![bad],
                    consumed: vec![event.id]
                }
            )
            .await,
        Err(StoreError::OutOfOrder { .. })
    ));
    let mut bad = entry.clone();
    bad.pos.epoch = Epoch(2);
    assert!(matches!(
        store
            .commit(
                session.id,
                Commit {
                    entries: vec![bad],
                    consumed: vec![event.id]
                }
            )
            .await,
        Err(StoreError::Fenced { .. })
    ));
    assert!(
        store
            .commit(
                session.id,
                Commit {
                    entries: vec![entry.clone()],
                    consumed: vec![]
                }
            )
            .await
            .is_err()
    );
    assert!(store.log(session.id, None).await.unwrap().is_empty());
    assert_eq!(
        store.pending(session.id).await.unwrap(),
        vec![event.clone()]
    );
    store
        .commit(
            session.id,
            Commit {
                entries: vec![entry.clone()],
                consumed: vec![event.id],
            },
        )
        .await
        .unwrap();
    assert!(store.pending(session.id).await.unwrap().is_empty());
    assert_eq!(
        store.log(session.id, None).await.unwrap(),
        vec![entry.clone()]
    );
    let node = store.node().await.unwrap().id;
    drop(store);
    let reopened = SqliteStore::open(dir.path().join("enco.db"), dir.path().join("blobs"))
        .await
        .unwrap();
    assert_eq!(reopened.node().await.unwrap().id, node);
    assert_eq!(reopened.log(session.id, None).await.unwrap(), vec![entry]);
    assert_eq!(reopened.accept(&event).await.unwrap(), Accepted::Duplicate);
}

#[tokio::test]
async fn reminder_firing_is_atomic_and_blob_reads_verify_their_address() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("enco.db"), dir.path().join("blobs"))
        .await
        .unwrap();
    let session = store
        .ensure_session("main", Utc::now().trunc_subsecs(3))
        .await
        .unwrap();
    let schedule = Schedule {
        id: ScheduleId::new(),
        session: session.id,
        due_at: Utc::now().trunc_subsecs(3),
        message: "remind".into(),
        created_at: Utc::now().trunc_subsecs(3),
        state: ScheduleState::Pending,
    };
    store.insert_schedule(&schedule).await.unwrap();
    let event = Event {
        id: EventId::new(),
        session: session.id,
        source: EventSource::Scheduler,
        body: EventBody::Reminder {
            schedule: schedule.id,
            due_at: schedule.due_at,
            text: schedule.message.clone(),
        },
        received_at: Utc::now().trunc_subsecs(3),
    };
    store.fire_schedule(schedule.id, &event).await.unwrap();
    assert!(matches!(
        store.fire_schedule(schedule.id, &event).await,
        Err(StoreError::ScheduleNotPending(_))
    ));
    assert_eq!(store.pending(session.id).await.unwrap(), vec![event]);
    let hash = store.put_blob(b"original").await.unwrap();
    assert_eq!(store.get_blob(&hash).await.unwrap(), b"original");
    std::fs::write(store.blob_path(&hash), b"changed").unwrap();
    assert!(matches!(
        store.get_blob(&hash).await,
        Err(StoreError::Blob(_))
    ));
}
