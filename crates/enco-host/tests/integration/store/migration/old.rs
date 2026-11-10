//! A version 23 database written table by table, with the identities the upgrade tests check.
use super::*;
use enco_core::code;
use enco_host::blob_path;
use serde_json::Value;

/// The plugins of the fixture registry: a factory composer, a registered chat Provider whose
/// second registration failed on trial, and a factory tool whose settings changed.
pub(super) struct Plugins {
    pub(super) composer: PluginId,
    pub(super) chat: PluginId,
    pub(super) tool: PluginId,
}

/// Facts a version 23 database holds, with the identities a test checks.
pub(super) struct Old {
    pub(super) node: NodeId,
    pub(super) plugins: Plugins,
    pub(super) main: SessionId,
    pub(super) child: SessionId,
    pub(super) old: SessionId,
    pub(super) kid: SessionId,
    pub(super) early: SessionId,
    pub(super) cron: ScheduleId,
    pub(super) by_agent: ScheduleId,
    pub(super) by_owner: ScheduleId,
    pub(super) once: ScheduleId,
    pub(super) received: Vec<u8>,
    pub(super) decoded: Vec<u8>,
    pub(super) failure: Failure,
}

pub(super) fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

pub(super) fn artifact(name: &str) -> ContentHash {
    ContentHash::of(name.as_bytes())
}

/// A service's response that a record read, which happens to look like a version 23 record:
/// only origins say what a record is.
pub(super) const RESPONSE: &[u8] =
    br#"{"generation":3,"export":"channel.received","arguments":{}}"#;

/// A plugin's own protocol state that happens to look like an origin.
pub(super) fn lookalike() -> Value {
    json!({ "call": "not a record", "index": 0 })
}

/// A version 23 mapping record, which named its code by a generation number.
pub(super) fn record(generation: u64, export: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "generation": generation,
        "export": export,
        "arguments": { "body": ContentHash::of(RESPONSE) },
    }))
    .unwrap()
}

/// The record version 24 keeps in its place.
pub(super) fn moved(code: CodeRef, export: &str) -> ContentHash {
    ContentHash::of(
        &serde_json::to_vec(&MappingRecord {
            code,
            export: export.into(),
            arguments: json!({ "body": ContentHash::of(RESPONSE) }),
        })
        .unwrap(),
    )
}

pub(super) fn origin(bytes: &[u8]) -> MappingOrigin {
    MappingOrigin {
        call: ContentHash::of(bytes),
        index: 0,
    }
}

pub(super) fn generation(registration: u64, plugin: PluginId) -> CodeRef {
    CodeRef::from(GenerationId {
        registration: RegistrationId(registration),
        plugin,
    })
}

/// An entry body in its version 24 form, patched back to how version 23 wrote it.
pub(super) fn old_form(body: EntryBody, patch: impl FnOnce(&mut Value)) -> String {
    let mut value = serde_json::to_value(body).unwrap();
    patch(&mut value);
    value.to_string()
}

pub(super) fn native() -> CodeRef {
    CodeRef::Native {
        name: "kernel-builtin".into(),
        version: "0.1.0".into(),
    }
}

pub(super) fn settled(call: CallId, content: &str) -> EntryBody {
    EntryBody::ToolCallSettled {
        call,
        outcome: Settlement::Ok,
        content: content.into(),
        full: None,
        images: Vec::new(),
        outgoing: None,
        delegated: None,
        schedule: None,
    }
}

/// What `schedule_create` returned for `schedule` in version 23.
pub(super) fn creation_result(schedule: ScheduleId) -> String {
    format!("{:#}", json!({ "id": schedule, "message": "check" }))
}

pub(super) fn started(round: RoundId, call: CallId, node: NodeId, name: &str) -> EntryBody {
    EntryBody::ToolCallStarted {
        round,
        call,
        capability: CapabilityId {
            node,
            name: name.into(),
        },
        code: native(),
        effect: Effect::SideEffect,
    }
}

/// A delegation that created its child, as version 23 recorded it.
pub(super) fn created(child: SessionId, name: &str) -> String {
    let body = EntryBody::ToolCallSettled {
        call: CallId::new(),
        outcome: Settlement::Ok,
        content: "Created the child.".into(),
        full: None,
        images: Vec::new(),
        outgoing: None,
        delegated: Some(Delegated {
            session: SessionRef {
                id: child,
                name: name.parse().unwrap(),
            },
            creation: None,
            event: EventId::new(),
        }),
        schedule: None,
    };
    old_form(body, |value| {
        let delegated = value["delegated"].as_object_mut().unwrap();
        delegated.remove("creation");
        delegated.insert("created".into(), json!(true));
    })
}

pub(super) fn reminder(session: SessionId, schedule: ScheduleId, due_at: &str) -> Event {
    Event {
        id: EventId::new(),
        session,
        source: EventSource::Scheduler,
        body: EventBody::Reminder {
            schedule,
            due_at: at(due_at),
            skipped: 0,
            text: "check".into(),
        },
        received_at: at(due_at),
    }
}

/// A rollback notice as version 23 recorded it, with generation numbers.
pub(super) fn rolled_back(
    session: SessionId,
    from: u64,
    to: Option<u64>,
    failure: &Failure,
) -> Value {
    let mut event = serde_json::to_value(Event {
        id: EventId::new(),
        session,
        source: EventSource::Registry,
        body: EventBody::PluginRolledBack {
            rollback: RolledBack {
                plugin: "chat".parse().unwrap(),
                from: GenerationId {
                    registration: RegistrationId(2),
                    plugin: PluginId::new(),
                },
                to: None,
                failure: failure.clone(),
            },
            text: "chat failed on trial and was rolled back.".into(),
        },
        received_at: at("2026-11-01T00:00:00Z"),
    })
    .unwrap();
    event["body"]["kind"] = json!("generation_rolled_back");
    event["body"]["rollback"]["from"] = json!(from);
    event["body"]["rollback"]["to"] = json!(to);
    event
}

impl Old {
    pub(super) fn new() -> Self {
        Self {
            node: NodeId::new(),
            plugins: Plugins {
                composer: PluginId::new(),
                chat: PluginId::new(),
                tool: PluginId::new(),
            },
            main: SessionId::new(),
            child: SessionId::new(),
            old: SessionId::new(),
            kid: SessionId::new(),
            early: SessionId::new(),
            cron: ScheduleId::new(),
            by_agent: ScheduleId::new(),
            by_owner: ScheduleId::new(),
            once: ScheduleId::new(),
            received: record(3, "channel.received"),
            decoded: record(3, "provider.decode"),
            failure: Failure {
                code: code::PLUGIN_TRAP.into(),
                message: "trapped".into(),
                retryable: false,
            },
        }
    }

    /// Write the database at `root`, with the blob holding the result the Log cut short.
    pub(super) fn write(&self, root: &Path) -> rusqlite::Connection {
        self.blob(root);
        let db = rusqlite::Connection::open(root.join("enco.db")).unwrap();
        // Rows go in table by table; the upgrade checks every reference.
        db.pragma_update(None, "foreign_keys", "OFF").unwrap();
        db.execute_batch(include_str!("../schema-23.sql")).unwrap();
        db.pragma_update(None, "user_version", 23).unwrap();
        db.execute(
            "INSERT INTO meta(key,value) VALUES ('node_id',?),('safe_mode','0')",
            [self.node.to_string()],
        )
        .unwrap();
        self.registry(&db);
        self.sessions(&db);
        self.logs(&db);
        self.schedules(&db);
        self.channel(&db);
        db
    }

    pub(super) fn blob(&self, root: &Path) {
        let result = creation_result(self.cron);
        let path = blob_path(&root.join("blobs"), &ContentHash::of(result.as_bytes()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, result).unwrap();
    }

    pub(super) fn registry(&self, db: &rusqlite::Connection) {
        let Plugins {
            composer,
            chat,
            tool,
        } = &self.plugins;
        db.execute_batch(
            "INSERT INTO registrations(id,initiator,created_at) VALUES
                 (1,'host','2026-10-01T00:00:00.000Z'),(2,'owner','2026-10-02T00:00:00.000Z');
             INSERT INTO generations(id,created_at) VALUES
                 (1,'t'),(2,'t'),(3,'t'),(4,'t'),(5,'t'),(6,'t'),(7,'t');",
        )
        .unwrap();
        let empty = serde_json::to_string(&PluginSettings::default()).unwrap();
        let changed = json!({ "config": { "depth": 2 }, "secrets": {} }).to_string();
        let failure = serde_json::to_string(&self.failure).unwrap();
        for (id, plugin, name, registration, origin, based_on, status, failure, settings) in [
            (
                1,
                composer,
                "composer factory",
                None,
                "factory",
                None,
                "healthy",
                None,
                &empty,
            ),
            (
                3,
                chat,
                "chat 1",
                Some(1),
                "code",
                None,
                "healthy",
                None,
                &empty,
            ),
            (
                4,
                tool,
                "tool factory",
                None,
                "factory",
                None,
                "healthy",
                None,
                &empty,
            ),
            (
                5,
                chat,
                "chat 2",
                Some(2),
                "code",
                None,
                "failed",
                Some(&failure),
                &empty,
            ),
            (
                6,
                tool,
                "tool factory",
                Some(2),
                "settings",
                Some(4),
                "trial",
                None,
                &changed,
            ),
        ] {
            db.execute(
                "INSERT INTO plugin_generations(id,plugin_id,artifact,settings,registration,origin,based_on,status,failure)
                 VALUES (?,?,?,?,?,?,?,?,?)",
                params![id, plugin.to_string(), artifact(name).to_string(), settings, registration, origin, based_on, status, failure],
            )
            .unwrap();
        }
        for (id, registration, previous) in [(2, 1, None), (7, 2, Some(2))] {
            db.execute(
                "INSERT INTO config_generations(id,registration,document,source,previous) VALUES (?,?,'{}',?,?)",
                params![id, registration, artifact("config").to_string(), previous],
            )
            .unwrap();
        }
        db.execute("INSERT INTO config(singleton,active) VALUES (1,7)", [])
            .unwrap();
        self.references(db);
    }

    /// Names, the code each registration activated, and this node's references.
    pub(super) fn references(&self, db: &rusqlite::Connection) {
        let Plugins {
            composer,
            chat,
            tool,
        } = &self.plugins;
        for (registration, plugin, name, previous) in [
            (1, composer, "composer", None),
            (1, chat, "chat", None),
            (1, tool, "tool", None),
            (2, tool, "tools", Some(1)),
        ] {
            db.execute(
                "INSERT INTO registration_names(registration,plugin_id,name,previous) VALUES (?,?,?,?)",
                params![registration, plugin.to_string(), name, previous],
            )
            .unwrap();
        }
        let source = artifact("source").to_string();
        for (registration, plugin, generation, intent, source) in [
            (1, composer, 1, Some("factory"), None),
            (1, chat, 3, Some("directory"), Some(&source)),
            (1, tool, 4, Some("factory"), None),
            (2, chat, 5, Some("directory"), Some(&source)),
            (2, tool, 6, None, None),
        ] {
            db.execute(
                "INSERT INTO registration_code(registration,plugin_id,generation,intent,source) VALUES (?,?,?,?,?)",
                params![registration, plugin.to_string(), generation, intent, source],
            )
            .unwrap();
        }
        // The chat Provider's second registration failed on trial and was rolled back.
        for (plugin, named_by, active, active_by, rollback, rollback_by) in [
            (composer, 1, 1, 1, None, None),
            (chat, 1, 3, 1, None, None),
            (tool, 2, 6, 2, Some(4), Some(1)),
        ] {
            db.execute(
                "INSERT INTO plugins(id,named_by,active,active_by,rollback_target,rollback_by) VALUES (?,?,?,?,?,?)",
                params![plugin.to_string(), named_by, active, active_by, rollback, rollback_by],
            )
            .unwrap();
        }
    }

    pub(super) fn sessions(&self, db: &rusqlite::Connection) {
        let node = self.node.to_string();
        for (id, name, parent, profile, path, archived) in [
            (self.main, "main", None, "default", "/work", None),
            (
                self.child,
                "main/research",
                Some(self.main),
                "research",
                "/work/research",
                None,
            ),
            (
                self.old,
                "old",
                None,
                "default",
                "/work",
                Some("2026-10-20T00:00:00.000Z"),
            ),
            (
                self.kid,
                "old/kid",
                Some(self.old),
                "default",
                "/work",
                Some("2026-10-20T00:00:00.000Z"),
            ),
            (
                self.early,
                "old/early",
                Some(self.old),
                "default",
                "/work",
                Some("2026-10-10T00:00:00.000Z"),
            ),
        ] {
            db.execute(
                "INSERT INTO sessions(id,name,created_at,binding_node,binding_epoch,profile,working_dir_node,working_dir_path,parent,archived_at)
                 VALUES (?,?,'2026-10-01T00:00:00.000Z',?,1,?,?,?,?,?)",
                params![id.to_string(), name, node, profile, node, path, parent.map(|id| id.to_string()), archived],
            )
            .unwrap();
        }
    }

    pub(super) fn append(&self, db: &rusqlite::Connection, session: SessionId, bodies: &[String]) {
        for (seq, body) in (1..).zip(bodies) {
            db.execute(
                "INSERT INTO log(session_id,epoch,seq,at,body) VALUES (?,1,?,'2026-11-01T00:00:00.000Z',?)",
                params![session.to_string(), seq, body],
            )
            .unwrap();
        }
    }

    /// One Round of `main` that creates four schedules, cancels one, delegates, calls a plugin
    /// tool and fails; then a rollback notice and two reminders.
    pub(super) fn logs(&self, db: &rusqlite::Connection) {
        db.execute(
            "INSERT INTO tool_catalogs(hash,body) VALUES ('catalog',x'00')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO context_plans(hash,catalog,body) VALUES (?,'catalog',x'00')",
            [ContentHash::of(b"plan").to_string()],
        )
        .unwrap();
        let (run, round) = (RunId::new(), RoundId::new());
        let creations =
            [self.cron, self.by_agent, self.by_owner, self.once].map(|id| (CallId::new(), id));
        let cancel = CallId::new();
        let mut bodies = self.reply(run, round, &creations, cancel);
        bodies.extend(self.settlements(round, &creations, cancel));
        bodies.extend([
            serde_json::to_string(&EntryBody::RunEnded {
                run,
                end: RunEnd::Completed,
            })
            .unwrap(),
            json!({ "kind": "event_consumed", "event": rolled_back(self.main, 5, Some(3), &self.failure) }).to_string(),
        ]);
        for event in [
            reminder(self.main, self.cron, "2026-11-02T01:00:00Z"),
            reminder(self.main, self.once, "2026-11-02T00:00:00Z"),
        ] {
            bodies.push(serde_json::to_string(&EntryBody::EventConsumed { event }).unwrap());
        }
        self.append(db, self.main, &bodies);
        self.append(
            db,
            self.old,
            &[
                created(self.kid, "old/kid"),
                created(self.early, "old/early"),
            ],
        );
        for event in [
            serde_json::to_value(reminder(self.main, self.cron, "2026-11-02T02:00:00Z")).unwrap(),
            rolled_back(self.main, 5, None, &self.failure),
        ] {
            db.execute(
                "INSERT INTO inbox(event_id,session_id,event) VALUES (?,?,?)",
                params![
                    event["id"].as_str().unwrap(),
                    self.main.to_string(),
                    event.to_string()
                ],
            )
            .unwrap();
        }
        for bytes in [self.received.as_slice(), &self.decoded, RESPONSE] {
            db.execute(
                "INSERT INTO evidence(hash,body) VALUES (?,?)",
                params![
                    ContentHash::of(bytes).to_string(),
                    zstd::stream::encode_all(bytes, 3).unwrap()
                ],
            )
            .unwrap();
        }
    }

    /// The channel input and the reply that proposes the scheduling calls.
    pub(super) fn reply(
        &self,
        run: RunId,
        round: RoundId,
        creations: &[(CallId, ScheduleId)],
        cancel: CallId,
    ) -> Vec<String> {
        let attempt = AttemptId::new();
        let input = Event {
            id: EventId::new(),
            session: self.main,
            source: EventSource::Channel {
                protocol: "telegram".into(),
                account: "bot".into(),
                conversation: "chat".into(),
                sender: "owner".into(),
                sender_name: None,
                owner: true,
                origin: origin(&self.received),
                message: "1".into(),
            },
            body: EventBody::UserMessage {
                text: "plan my week".into(),
                attachments: Vec::new(),
                unviewable: Vec::new(),
                earlier: Vec::new(),
                quote: None,
            },
            received_at: at("2026-11-01T00:00:00Z"),
        };
        let calls: Vec<Part> = creations
            .iter()
            .map(|(call, _)| {
                (
                    *call,
                    "schedule_create",
                    json!({ "cron": "0 * * * *", "message": "check" }),
                )
            })
            .chain([(cancel, "schedule_cancel", json!({ "id": self.by_agent }))])
            .map(|(id, name, arguments)| {
                Part::ToolCall(ToolCall {
                    id,
                    provider_id: id.to_string(),
                    name: name.into(),
                    arguments: arguments.to_string(),
                })
            })
            .collect();
        vec![
            serde_json::to_string(&EntryBody::EventConsumed { event: input }).unwrap(),
            serde_json::to_string(&EntryBody::RunStarted { run }).unwrap(),
            serde_json::to_string(&EntryBody::RoundStarted {
                run,
                round,
                safe_mode: false,
                working_dir: WorkingDir::new(self.node, "/work".into()).unwrap(),
            })
            .unwrap(),
            old_form(
                EntryBody::AttemptStarted {
                    round,
                    attempt,
                    purpose: AttemptPurpose::Reply,
                    plan: ContentHash::of(b"plan"),
                    composer: native(),
                    provider: native(),
                    settings: serde_json::from_value(json!({
                        "base_url": "https://fixture.invalid",
                        "model": "fixture",
                        "api_key_env": null,
                        "options": {},
                    }))
                    .unwrap(),
                },
                |value| {
                    value["composer"] = json!(1);
                    value["provider"] = json!(3);
                },
            ),
            serde_json::to_string(&EntryBody::AttemptSettled {
                attempt,
                result: AttemptResult::Completed {
                    message: Message {
                        role: Role::Assistant,
                        parts: calls,
                    },
                    usage: Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                        cached_input_tokens: None,
                    },
                    stop: StopReason::ToolCalls,
                },
                origin: Some(origin(&self.decoded)),
            })
            .unwrap(),
        ]
    }

    /// The Round's calls as they settled, a delegation and a plugin tool among them, and its
    /// failure.
    pub(super) fn settlements(
        &self,
        round: RoundId,
        creations: &[(CallId, ScheduleId)],
        cancel: CallId,
    ) -> Vec<String> {
        let tool = CallId::new();
        let mut bodies = Vec::new();
        for (call, schedule) in creations {
            bodies.push(
                serde_json::to_string(&started(round, *call, self.node, "schedule_create"))
                    .unwrap(),
            );
            let result = creation_result(*schedule);
            let mut body = settled(*call, &result);
            // Too long for the Log, this result left a cut preview there and its whole in a blob.
            if *schedule == self.cron
                && let EntryBody::ToolCallSettled { content, full, .. } = &mut body
            {
                (*content, *full) = ("{".into(), Some(ContentHash::of(result.as_bytes())));
            }
            bodies.push(serde_json::to_string(&body).unwrap());
        }
        bodies.extend([
            serde_json::to_string(&started(round, cancel, self.node, "schedule_cancel")).unwrap(),
            serde_json::to_string(&settled(
                cancel,
                &format!("{:#}", json!({ "cancelled": true })),
            ))
            .unwrap(),
            created(self.child, "main/research"),
            old_form(started(round, tool, self.node, "tool_run"), |value| {
                value["code"] = json!({ "kind": "generation", "id": 6 });
            }),
            serde_json::to_string(&settled(tool, "ran")).unwrap(),
            old_form(
                EntryBody::RoundEnded {
                    round,
                    end: RoundEnd::Failed {
                        failure: self.failure.clone(),
                        active: ActiveCode {
                            configuration: RegistrationId(2),
                            plugins: BTreeMap::new(),
                        },
                    },
                },
                |value| {
                    value["end"] = json!({
                        "kind": "failed",
                        "failure": self.failure,
                        "generations": [1, 3, 6, 7],
                    });
                },
            ),
        ]);
        bodies
    }

    /// The schedules as version 23 kept them; the pending reminder already counts.
    pub(super) fn schedules(&self, db: &rusqlite::Connection) {
        let pending: String = db
            .query_row(
                "SELECT event_id FROM inbox WHERE json_extract(event,'$.body.kind')='reminder'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let consumed: String = db
            .query_row(
                "SELECT event_id FROM log WHERE json_extract(body,'$.event.body.schedule')=?",
                [self.once.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let cron = json!({ "kind": "cron", "expr": "0 * * * *", "timezone": null }).to_string();
        let once = json!({ "kind": "once", "at": "2026-11-02T00:00:00Z" }).to_string();
        for (id, rule, state, last_due, last_event) in [
            (
                self.cron,
                &cron,
                "active",
                Some("2026-11-02T02:00:00.000Z"),
                Some(pending),
            ),
            (self.by_agent, &cron, "cancelled", None, None),
            (self.by_owner, &cron, "cancelled", None, None),
            (
                self.once,
                &once,
                "done",
                Some("2026-11-02T00:00:00.000Z"),
                Some(consumed),
            ),
        ] {
            db.execute(
                "INSERT INTO schedules VALUES (?,?,?,'check','2026-11-01T00:00:00.000Z',?,?,?)",
                params![
                    id.to_string(),
                    self.main.to_string(),
                    rule,
                    state,
                    last_due,
                    last_event
                ],
            )
            .unwrap();
        }
    }

    /// A connection that quotes the channel input and settled one delivery after a failure.
    pub(super) fn channel(&self, db: &rusqlite::Connection) {
        let earlier = EarlierMessage {
            received_at: at("2026-11-01T00:00:00Z"),
            sender: None,
            sender_name: None,
            owner: true,
            text: "hi".into(),
            attachments: Vec::new(),
            origin: origin(&self.received),
            quote: None,
        };
        let state = json!({
            "protocol": lookalike(),
            "chats": {},
            "context": { "chat": [earlier] },
            "everyone": [],
            "outbound": {},
            "sending": null,
            "children": {},
            "activity": {},
        });
        db.execute(
            "INSERT INTO connections(key,state) VALUES ('telegram:bot',?)",
            [state.to_string()],
        )
        .unwrap();
        let settlement = DeliverySettlement {
            delivery: Delivery {
                session: self.main,
                pos: LogPos {
                    epoch: Epoch(1),
                    seq: Seq(1),
                },
                target: "chat".into(),
            },
            outcome: Settlement::Failed {
                failure: self.failure.clone(),
            },
            messages: Vec::new(),
            origins: vec![origin(&self.received)],
            at: at("2026-11-01T00:00:00Z"),
        };
        db.execute(
            "INSERT INTO deliveries(connection,session_id,epoch,seq,body) VALUES ('telegram:bot',?,1,1,?)",
            params![self.main.to_string(), serde_json::to_string(&settlement).unwrap()],
        )
        .unwrap();
    }
}
