//! A version 23 database opens upgraded to version 24: code is named by registration or as
//! factory code, Session and schedule rows rebuild from facts recorded where they belong, mapping
//! records move with the origins that point at them, and a database whose facts do not rebuild is
//! left as it was.
use super::*;
use rusqlite::params;
use serde_json::json;
use std::collections::BTreeMap;

mod old;
use old::*;

fn gid(registration: u64, plugin: PluginId) -> GenerationId {
    GenerationId {
        registration: RegistrationId(registration),
        plugin,
    }
}

fn factory(name: &str) -> PluginCode {
    PluginCode::Factory {
        artifact: artifact(name),
    }
}

/// The copies of the database taken before upgrading it.
fn copies(root: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("enco.db.v23-")
        })
        .collect()
}

fn version(path: &Path) -> u32 {
    rusqlite::Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

async fn upgraded(old: &Old) -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    drop(old.write(dir.path()));
    let store = support::open_store(dir.path()).await;
    (dir, store)
}

fn find<T>(log: &[Entry], pick: impl FnMut(&EntryBody) -> Option<T>) -> T {
    log.iter().map(|entry| &entry.body).find_map(pick).unwrap()
}

#[tokio::test]
async fn the_upgraded_registry_names_code_by_registration_or_as_factory_code() {
    let old = Old::new();
    let (_dir, store) = upgraded(&old).await;
    let Plugins {
        composer,
        chat,
        tool,
    } = old.plugins;
    let registry = store.registry().await.unwrap();
    let mut brought = vec![gid(1, chat), gid(2, chat), gid(2, tool)];
    brought.sort();
    assert_eq!(
        registry
            .intent
            .generations
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        brought
    );
    assert_eq!(
        registry.intent.generations[&gid(2, tool)].origin,
        Origin::Settings {
            based_on: factory("tool factory")
        }
    );
    assert_eq!(
        registry.intent.registrations[&RegistrationId(1)].code[&composer],
        RegisteredCode::Factory {
            artifact: artifact("composer factory")
        }
    );
    let reference = |plugin: PluginId, rollback: bool| {
        let record = &registry.node.plugins[&plugin];
        if rollback {
            record.rollback_target
        } else {
            record.active
        }
    };
    assert_eq!(
        reference(chat, false),
        Some(PluginActivation::Generation {
            registration: RegistrationId(1)
        })
    );
    assert_eq!(
        reference(tool, true),
        Some(PluginActivation::Factory {
            artifact: artifact("tool factory"),
            registration: Some(RegistrationId(1))
        })
    );
    assert_eq!(
        registry.node.health[&gid(2, chat)],
        Health::Failed {
            failure: old.failure.clone()
        }
    );
    assert_eq!(registry.node.configuration, Some(RegistrationId(2)));
    assert_eq!(
        registry.node.replaced[&RegistrationId(2)],
        enco_kernel::Replaced {
            names: BTreeMap::from([(tool, Some(RegistrationId(1)))]),
            configuration: Some(RegistrationId(1)),
        }
    );
}

#[tokio::test]
async fn upgraded_logs_and_inboxes_name_the_same_code() {
    let old = Old::new();
    let (_dir, store) = upgraded(&old).await;
    let Plugins {
        composer,
        chat,
        tool,
    } = old.plugins;
    let log = store.log(old.main, None).await.unwrap();
    let (composed_by, provided_by) = find(&log, |body| match body {
        EntryBody::AttemptStarted {
            composer, provider, ..
        } => Some((*composer, *provider)),
        _ => None,
    });
    assert_eq!(
        composed_by,
        CodeRef {
            plugin: composer,
            code: factory("composer factory")
        }
    );
    assert_eq!(provided_by, generation(1, chat));
    let ran = find(&log, |body| match body {
        EntryBody::ToolCallStarted {
            capability, code, ..
        } if capability.name == "tool_run" => Some(code.clone()),
        _ => None,
    });
    assert_eq!(ran, ToolCode::Plugin(generation(2, tool)));
    let failed_with = find(&log, |body| match body {
        EntryBody::RoundEnded {
            end: RoundEnd::Failed { active, .. },
            ..
        } => Some(active.clone()),
        _ => None,
    });
    assert_eq!(
        failed_with,
        ActiveCode {
            configuration: RegistrationId(2),
            plugins: BTreeMap::from([
                (composer, factory("composer factory")),
                (chat, generation(1, chat).code),
                (tool, generation(2, tool).code),
            ]),
        }
    );
    let rollback = |event: &Event| match &event.body {
        EventBody::PluginRolledBack { rollback, text } => Some((rollback.clone(), text.clone())),
        _ => None,
    };
    let (consumed, text) = find(&log, |body| match body {
        EntryBody::EventConsumed { event } => rollback(event),
        _ => None,
    });
    assert_eq!(
        (consumed.from, consumed.to),
        (gid(2, chat), Some(generation(1, chat).code))
    );
    assert_eq!(text, "chat failed on trial and was rolled back.");
    let pending = store.pending(old.main).await.unwrap();
    let (pending, _) = pending.iter().find_map(rollback).unwrap();
    assert_eq!((pending.from, pending.to), (gid(2, chat), None));
}

#[tokio::test]
async fn upgraded_mapping_records_move_and_every_origin_follows() {
    let old = Old::new();
    let (dir, store) = upgraded(&old).await;
    let chat = old.plugins.chat;
    let log = store.log(old.main, None).await.unwrap();
    let received = moved(generation(1, chat), "channel.received");
    let decoded = moved(generation(1, chat), "provider.decode");
    let input = find(&log, |body| match body {
        EntryBody::EventConsumed {
            event:
                Event {
                    source: EventSource::Channel { origin, .. },
                    ..
                },
        } => Some(origin.call),
        _ => None,
    });
    assert_eq!(input, received);
    let replied = find(&log, |body| match body {
        EntryBody::AttemptSettled { origin, .. } => *origin,
        _ => None,
    });
    assert_eq!(replied.call, decoded);
    let record: MappingRecord =
        serde_json::from_slice(&store.evidence(&received).await.unwrap()).unwrap();
    assert_eq!(record.code, generation(1, chat));
    assert!(
        store
            .evidence(&ContentHash::of(&old.received))
            .await
            .is_err()
    );
    let delivered = store.delivery_failures("telegram:bot").await.unwrap();
    assert_eq!(delivered[0].origins[0].call, received);
    let state = store.connection("telegram:bot").await.unwrap().unwrap();
    assert_eq!(
        state["context"]["chat"][0]["origin"]["call"],
        json!(received)
    );
    // What looks like a record or an origin in content a service or plugin owns stays as it was.
    assert_eq!(state["protocol"], lookalike());
    assert_eq!(
        store.evidence(&ContentHash::of(RESPONSE)).await.unwrap(),
        RESPONSE
    );
    // The copy keeps the database as it was.
    assert_eq!(version(&dir.path().join("enco.db")), 24);
    assert_eq!(version(&copies(dir.path())[0]), 23);
}

#[tokio::test]
async fn upgraded_sessions_and_schedules_rebuild_from_their_own_facts() {
    let old = Old::new();
    let (dir, store) = upgraded(&old).await;
    let db = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
    let ids = |sql: &str| -> Vec<(String, Option<String>)> {
        db.prepare(sql)
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    // Roots are in the root directory, and a child's creation is in its parent's Log.
    let mut roots = vec![(old.main.to_string(), None), (old.old.to_string(), None)];
    roots.sort();
    assert_eq!(
        ids("SELECT id, NULL FROM root_directory ORDER BY id"),
        roots
    );
    let log = store.log(old.main, None).await.unwrap();
    let child = find(&log, |body| match body {
        EntryBody::ToolCallSettled {
            delegated:
                Some(Delegated {
                    target: DelegationTarget::Created(creation),
                    ..
                }),
            ..
        } => Some(creation.clone()),
        _ => None,
    });
    assert_eq!(
        (
            child.id,
            child.profile.as_str(),
            child.working_dir.path_str()
        ),
        (old.child, "research", "/work/research")
    );
    // Only the archives that an ancestor's archive does not imply are recorded.
    let mut archives = vec![
        (old.old.to_string(), Some("2026-10-20T00:00:00.000Z".into())),
        (
            old.early.to_string(),
            Some("2026-10-10T00:00:00.000Z".into()),
        ),
    ];
    archives.sort();
    assert_eq!(
        ids("SELECT session_id, at FROM archives ORDER BY session_id"),
        archives
    );
    let kid = store.session(old.kid).await.unwrap().unwrap();
    assert_eq!(kid.archived_at, Some(at("2026-10-20T00:00:00Z")));
    // Each schedule's creation and the Agent's own cancel are on their settlements; the owner's
    // cancel is a command at the end of the Log.
    let changes: Vec<ScheduleChange> = log
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::ToolCallSettled { schedule, .. } => schedule.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 5);
    assert_eq!(changes[4], ScheduleChange::Cancelled { id: old.by_agent });
    assert!(matches!(
        &log.last().unwrap().body,
        EntryBody::EventConsumed { event }
            if event.body == EventBody::CancelSchedule { schedule: old.by_owner }
    ));
    // The pending reminder counts in the effective view only, not in the Log's facts.
    let last = |rows: Vec<Schedule>, id| {
        let row = rows.into_iter().find(|row| row.id == id).unwrap();
        (row.state, row.last.map(|last| last.due_at))
    };
    let mut committed = ScheduleFold::new(old.main);
    for entry in &log {
        committed.apply(&entry.body);
    }
    assert_eq!(
        last(committed.rows(), old.cron),
        (ScheduleState::Active, Some(at("2026-11-02T01:00:00Z")))
    );
    let effective = store.schedules(Some(old.main)).await.unwrap();
    assert_eq!(
        last(effective.clone(), old.cron),
        (ScheduleState::Active, Some(at("2026-11-02T02:00:00Z")))
    );
    assert_eq!(
        last(effective, old.once),
        (ScheduleState::Done, Some(at("2026-11-02T00:00:00Z")))
    );
}

/// The call that created a schedule never settled: version 23 changed its table first.
fn unsettled(old: &Old, db: &rusqlite::Connection) -> String {
    db.execute(
        "UPDATE log SET body=json_set(body,'$.content','{}')
         WHERE json_extract(body,'$.content') LIKE ?",
        [format!("%{}%", old.once)],
    )
    .unwrap();
    old.once.to_string()
}

/// The channel input's origin points at a response instead of the record that read it.
fn misdirected(old: &Old, db: &rusqlite::Connection) -> String {
    let [record, response] = [old.received.as_slice(), b"{\"status\":200}"].map(ContentHash::of);
    db.execute(
        "INSERT INTO evidence(hash,body) VALUES (?,?)",
        params![
            response.to_string(),
            zstd::stream::encode_all(&b"{\"status\":200}"[..], 3).unwrap()
        ],
    )
    .unwrap();
    db.execute(
        "UPDATE log SET body=replace(body,?,?)",
        [record.to_string(), response.to_string()],
    )
    .unwrap();
    response.to_string()
}

#[tokio::test]
async fn a_database_whose_facts_do_not_rebuild_is_left_unchanged() {
    for break_facts in [unsettled, misdirected] {
        let old = Old::new();
        let dir = tempfile::tempdir().unwrap();
        let db = old.write(dir.path());
        let named = break_facts(&old, &db);
        let contents = |db: &rusqlite::Connection| -> Vec<String> {
            db.prepare(
                "SELECT body FROM log UNION ALL SELECT sql FROM sqlite_master WHERE sql IS NOT NULL
                 UNION ALL SELECT hash FROM evidence",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
        };
        let before = contents(&db);
        drop(db);
        let error = SqliteStore::open(
            support::store_paths(dir.path()),
            dir.path().join("workspace"),
        )
        .await
        .err()
        .unwrap();
        assert!(
            matches!(&error, StoreError::Migration { from: 23, reason } if reason.contains(&named)),
            "{error}"
        );
        let db = rusqlite::Connection::open(dir.path().join("enco.db")).unwrap();
        assert_eq!(version(&dir.path().join("enco.db")), 23);
        assert_eq!(contents(&db), before);
        assert_eq!(copies(dir.path()).len(), 1);
    }
}

/// Every row of every table, in a stable order.
fn rows(root: &Path) -> Vec<String> {
    let db = rusqlite::Connection::open(root.join("enco.db")).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut all = Vec::new();
    for table in tables {
        let mut query = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = query.column_count();
        let mut rows = query.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let values: Vec<String> = (0..columns)
                .map(|column| format!("{:?}", row.get_ref(column).unwrap()))
                .collect();
            all.push(format!("{table}: {}", values.join(" | ")));
        }
    }
    all
}

#[tokio::test]
async fn upgrading_the_same_database_twice_writes_the_same_rows() {
    let old = Old::new();
    let [first, second] = [(); 2].map(|()| tempfile::tempdir().unwrap());
    drop(old.write(first.path()));
    old.blob(second.path());
    std::fs::copy(first.path().join("enco.db"), second.path().join("enco.db")).unwrap();
    for dir in [&first, &second] {
        drop(support::open_store(dir.path()).await);
    }
    assert_eq!(rows(first.path()), rows(second.path()));
}
