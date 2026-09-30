use chrono::SubsecRound;
use enco_core::*;
use enco_host::SqliteStore;
use enco_kernel::{Accepted, Commit, ConnectionWrite, Store, StoreError};

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
    assert_eq!(
        store
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap(),
        vec![Accepted::New]
    );
    assert_eq!(
        store
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap(),
        vec![Accepted::Duplicate]
    );
    let write = ConnectionWrite {
        key: "channel:account".into(),
        state: serde_json::json!({"offset": 1}),
        settlement: None,
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
    assert_eq!(
        store.pending(session.id).await.unwrap(),
        vec![event.clone()]
    );
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
                session.id,
                Commit {
                    entries: vec![entry.clone(), second],
                    consumed: vec![event.id, missing.id]
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
    assert_eq!(
        reopened
            .accept(std::slice::from_ref(&event), None)
            .await
            .unwrap(),
        vec![Accepted::Duplicate]
    );
}

#[tokio::test]
async fn blob_reads_reject_content_that_no_longer_matches_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("enco.db"), dir.path().join("blobs"))
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
