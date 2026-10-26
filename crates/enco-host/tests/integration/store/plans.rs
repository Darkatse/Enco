use super::*;
use enco_kernel::PlanWrite;
use rusqlite::{Connection, params};
use std::sync::Arc;

fn plan(node: NodeId) -> ContextPlan {
    ContextPlan {
        items: vec![inline("Standing instructions".repeat(4_000))],
        tools: vec![(
            CapabilityId {
                node,
                name: "read".into(),
            },
            DisclosedTool {
                name: "read".into(),
                description: "Read a file".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false}),
                effect: Effect::ReadOnly,
            },
        )],
        max_output_tokens: Some(1_024),
        omitted: Vec::new(),
    }
}

fn inline(text: String) -> PlanItem {
    PlanItem::Message {
        message: Message::text(Role::System, text),
        sources: Vec::new(),
    }
}

fn attempt(seq: u64, hash: ContentHash) -> Entry {
    Entry {
        pos: LogPos {
            epoch: Epoch(1),
            seq: Seq(seq),
        },
        at: Utc::now().trunc_subsecs(3),
        body: EntryBody::AttemptStarted {
            round: RoundId::new(),
            attempt: AttemptId::new(),
            purpose: AttemptPurpose::Reply,
            plan: hash,
            composer: GenerationId(1),
            provider: GenerationId(2),
            settings: ProviderSettings {
                base_url: "https://example.test".into(),
                model: "fixture".into(),
                api_key_env: None,
                options: serde_json::json!({}),
            },
        },
    }
}

async fn save(store: &SqliteStore, session: SessionId, seq: u64, plan: ContextPlan) -> ContentHash {
    let write = Arc::new(PlanWrite::new(plan).unwrap());
    let hash = write.hash();
    store
        .commit(
            session,
            Commit {
                entries: vec![attempt(seq, hash)],
                plan: Some(write),
            },
        )
        .await
        .unwrap();
    hash
}

#[tokio::test]
async fn plans_share_catalogs_and_prefixes_but_bound_restoration_work() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    let mut plan = plan(store.node().await.unwrap().id);
    let mut saved = Vec::new();
    for seq in 1..=66 {
        plan.items.push(inline(format!("New message {seq}")));
        plan.max_output_tokens = Some(1_024 + seq as u32);
        let hash = save(&store, event.session, seq, plan.clone()).await;
        saved.push((hash, plan.clone()));
    }
    let db = Connection::open(dir.path().join("enco.db")).unwrap();
    let catalogs: i64 = db
        .query_row("SELECT COUNT(*) FROM tool_catalogs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(catalogs, 1);
    // Restoring every plan below fails past the chain limit; shared prefixes leave few full plans.
    let full: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM context_plans WHERE parent IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(full < i64::try_from(saved.len()).unwrap());
    let delta_bytes: i64 = db
        .query_row(
            "SELECT MAX(length(body)) FROM context_plans WHERE parent IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(delta_bytes < 256);
    plan.items.reverse();
    let replaced = save(&store, event.session, 67, plan.clone()).await;
    let parent: Option<String> = db
        .query_row(
            "SELECT parent FROM context_plans WHERE hash=?",
            [replaced.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(parent.is_none());
    saved.push((replaced, plan));
    drop(store);
    let reopened = SqliteStore::open(support::store_paths(dir.path()))
        .await
        .unwrap();
    for (hash, expected) in saved {
        assert_eq!(reopened.plan(&hash).await.unwrap(), expected);
    }
    reopened
        .commit(
            event.session,
            Commit {
                entries: vec![attempt(68, replaced)],
                plan: None,
            },
        )
        .await
        .unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM context_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 67);
}

#[tokio::test]
async fn a_failed_attempt_insert_rolls_back_the_plan_and_its_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    let write = Arc::new(PlanWrite::new(plan(store.node().await.unwrap().id)).unwrap());
    let hash = write.hash();
    let db = Connection::open(dir.path().join("enco.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_attempt BEFORE INSERT ON log BEGIN SELECT RAISE(ABORT,'injected attempt failure'); END;").unwrap();
    assert!(
        store
            .commit(
                event.session,
                Commit {
                    entries: vec![attempt(1, hash)],
                    plan: Some(write)
                }
            )
            .await
            .is_err()
    );
    let count: i64 = db.query_row("SELECT (SELECT COUNT(*) FROM context_plans)+(SELECT COUNT(*) FROM tool_catalogs)+(SELECT COUNT(*) FROM log)", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
    assert!(matches!(
        store.plan(&hash).await,
        Err(StoreError::Plan { .. })
    ));
}

#[tokio::test]
async fn plan_reads_reject_changed_content_and_missing_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let (store, event) = main_session(dir.path()).await;
    let mut plan = plan(store.node().await.unwrap().id);
    let first = save(&store, event.session, 1, plan.clone()).await;
    plan.items.push(inline("Later input".into()));
    let second = save(&store, event.session, 2, plan).await;
    let db = Connection::open(dir.path().join("enco.db")).unwrap();
    let original: Vec<u8> = db
        .query_row(
            "SELECT body FROM context_plans WHERE hash=?",
            [first.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let changed = zstd::stream::encode_all(
        br#"{"items":[],"max_output_tokens":1024,"omitted":[]}"#.as_slice(),
        3,
    )
    .unwrap();
    db.execute(
        "UPDATE context_plans SET body=? WHERE hash=?",
        params![changed, first.to_string()],
    )
    .unwrap();
    assert!(
        matches!(store.plan(&second).await, Err(StoreError::Plan { hash, .. }) if hash == second)
    );
    db.execute(
        "UPDATE context_plans SET body=? WHERE hash=?",
        params![original, first.to_string()],
    )
    .unwrap();
    db.pragma_update(None, "foreign_keys", false).unwrap();
    db.execute("DELETE FROM tool_catalogs", []).unwrap();
    assert!(matches!(
        store.plan(&second).await,
        Err(StoreError::Plan { .. })
    ));
    db.execute(
        "DELETE FROM context_plans WHERE hash=?",
        [first.to_string()],
    )
    .unwrap();
    assert!(matches!(
        store.plan(&second).await,
        Err(StoreError::Plan { .. })
    ));
}

#[tokio::test]
async fn fresh_databases_enable_incremental_vacuum_and_old_versions_stay_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = main_session(dir.path()).await;
    let db = Connection::open(dir.path().join("enco.db")).unwrap();
    let mode: i64 = db
        .pragma_query_value(None, "auto_vacuum", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, 2);
    db.pragma_update(None, "user_version", 12).unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::open(support::store_paths(dir.path())).await,
        Err(StoreError::SchemaVersion { found: 12, .. })
    ));
    let sessions: i64 = db
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sessions, 1);
    let version: i64 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 12);
}
