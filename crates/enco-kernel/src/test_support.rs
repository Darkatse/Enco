use enco_core::*;

/// The Session that every fixture event and record belongs to.
pub(crate) fn session_id() -> SessionId {
    "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap()
}

pub(crate) fn event(body: EventBody) -> Event {
    Event {
        id: EventId::new(),
        session: session_id(),
        source: EventSource::Cli,
        body,
        received_at: Utc::now(),
    }
}

pub(crate) fn entries(bodies: Vec<EntryBody>) -> Vec<Entry> {
    bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| Entry {
            pos: LogPos {
                epoch: Epoch(1),
                seq: Seq(index as u64 + 1),
            },
            at: Utc::now(),
            body,
        })
        .collect()
}

pub(crate) fn consumed(body: EventBody) -> EntryBody {
    EntryBody::EventConsumed { event: event(body) }
}

pub(crate) fn message() -> EntryBody {
    consumed(EventBody::UserMessage {
        text: "task".into(),
        attachments: vec![],
        unviewable: vec![],
        earlier: vec![],
        quote: None,
    })
}

pub(crate) fn round_end() -> EntryBody {
    EntryBody::RoundEnded {
        round: RoundId::new(),
        end: RoundEnd::Replied,
    }
}

pub(crate) fn started(attempt: AttemptId, purpose: AttemptPurpose, model: &str) -> EntryBody {
    EntryBody::AttemptStarted {
        round: RoundId::new(),
        attempt,
        purpose,
        plan: ContentHash::of(b"plan"),
        composer: GenerationId(1),
        provider: GenerationId(1),
        settings: settings(model),
    }
}

/// The call parameters `started` records for `model`.
pub(crate) fn settings(model: &str) -> ProviderSettings {
    ProviderSettings {
        base_url: "https://example.test".parse().unwrap(),
        model: model.into(),
        api_key_env: None,
        options: serde_json::json!({}),
    }
}

pub(crate) fn completed(attempt: AttemptId, input_tokens: u64) -> EntryBody {
    EntryBody::AttemptSettled {
        origin: None,
        attempt,
        result: AttemptResult::Completed {
            message: Message {
                role: Role::Assistant,
                parts: vec![Part::Text {
                    text: "reply".into(),
                }],
            },
            usage: Usage {
                input_tokens,
                output_tokens: 20,
                cached_input_tokens: Some(10),
            },
            stop: StopReason::EndTurn,
        },
    }
}

pub(crate) fn compacted(upto: LogPos) -> EntryBody {
    EntryBody::Compacted {
        upto,
        summary: "summary".into(),
        attempt: AttemptId::new(),
    }
}

pub(crate) fn halt() -> EventBody {
    EventBody::Halt(owner_halt())
}

/// The owner halting the fixture Session.
pub(crate) fn owner_halt() -> Halt {
    Halt {
        root: SessionRef {
            id: session_id(),
            name: "main".parse().unwrap(),
        },
        by: None,
    }
}

/// A Log built the way Session actors commit one, a Run or a command at a time.
#[derive(Default)]
pub(crate) struct History(pub Vec<Entry>);

impl History {
    pub fn push(&mut self, body: EntryBody) -> LogPos {
        let pos = LogPos {
            epoch: Epoch(1),
            seq: Seq(self.0.len() as u64 + 1),
        };
        self.0.push(Entry {
            pos,
            at: Utc::now(),
            body,
        });
        pos
    }

    /// Consume an input, between Runs or as a Round starts.
    pub fn input(&mut self, body: EventBody) -> LogPos {
        self.push(consumed(body))
    }

    pub fn run(&mut self) -> RunId {
        let run = RunId::new();
        self.push(EntryBody::RunStarted { run });
        run
    }

    pub fn end_run(&mut self, run: RunId, end: RunEnd) {
        self.push(EntryBody::RunEnded { run, end });
    }

    pub fn round(&mut self, run: RunId) -> RoundId {
        let round = RoundId::new();
        self.push(EntryBody::RoundStarted {
            run,
            round,
            safe_mode: false,
            working_dir: WorkingDir::new(NodeId::new(), "/tmp".into()).unwrap(),
        });
        round
    }

    pub fn end_round(&mut self, round: RoundId, end: RoundEnd) -> LogPos {
        self.push(EntryBody::RoundEnded { round, end })
    }

    /// A reply request the service answers; returns where the answer settled.
    pub fn reply(&mut self) -> LogPos {
        let attempt = AttemptId::new();
        self.push(started(attempt, AttemptPurpose::Reply, "reply-model"));
        self.push(completed(attempt, 100))
    }

    /// A reply request the service turns away as too large, giving `accepted` as its limit.
    pub fn refused(&mut self, accepted: u64) {
        let attempt = AttemptId::new();
        self.push(started(attempt, AttemptPurpose::Reply, "reply-model"));
        self.push(EntryBody::AttemptSettled {
            origin: None,
            attempt,
            result: AttemptResult::Overflow {
                overflow: Overflow {
                    counted: None,
                    accepted: Some(accepted),
                    message: "too long".into(),
                },
            },
        });
    }

    /// A reply that proposes one tool call, and the call starting; `settled` settles it too.
    pub fn call(&mut self, round: RoundId, settled: bool) {
        let attempt = AttemptId::new();
        let call = ToolCall {
            id: CallId::new(),
            provider_id: String::new(),
            name: "tool".into(),
            arguments: "{}".into(),
        };
        self.push(started(attempt, AttemptPurpose::Reply, "reply-model"));
        self.push(EntryBody::AttemptSettled {
            origin: None,
            attempt,
            result: AttemptResult::Completed {
                message: Message {
                    role: Role::Assistant,
                    parts: vec![Part::ToolCall(call.clone())],
                },
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 20,
                    cached_input_tokens: None,
                },
                stop: StopReason::ToolCalls,
            },
        });
        self.push(EntryBody::ToolCallStarted {
            round,
            call: call.id,
            capability: CapabilityId {
                node: NodeId::new(),
                name: call.name,
            },
            code: CodeRef::Generation {
                id: GenerationId(1),
            },
            effect: Effect::ReadOnly,
        });
        if settled {
            self.push(EntryBody::ToolCallSettled {
                call: call.id,
                outcome: Settlement::Ok,
                content: "done".into(),
                full: None,
                images: vec![],
                outgoing: None,
                delegated: None,
            });
        }
    }

    /// A Run of one Round that replies to `text`; returns where it consumed the input and where
    /// the Round ended.
    pub fn answered(&mut self, text: &str) -> (LogPos, LogPos) {
        let run = self.run();
        let input = self.input(said(text));
        let round = self.round(run);
        self.reply();
        let end = self.end_round(round, RoundEnd::Replied);
        self.end_run(run, RunEnd::Completed);
        (input, end)
    }

    /// A summary up to `upto`, recorded as a compaction.
    pub fn compact(&mut self, upto: LogPos) {
        let attempt = AttemptId::new();
        self.push(started(attempt, AttemptPurpose::Compaction, "reply-model"));
        self.push(completed(attempt, 100));
        self.push(EntryBody::Compacted {
            upto,
            summary: format!("summary to {}", upto.seq.0),
            attempt,
        });
    }
}

pub(crate) fn said(text: &str) -> EventBody {
    EventBody::UserMessage {
        text: text.into(),
        attachments: vec![],
        unviewable: vec![],
        earlier: vec![],
        quote: None,
    }
}

/// A Log that passes through what moves the reads' stops: Runs of several Rounds, compactions
/// between and within Runs, skipped replies, a refusal with a limit, backtracks to a reply before
/// two compactions, to the start and to an input, a halt, and a Round the process left unsettled.
pub(crate) fn history() -> Vec<Entry> {
    let mut h = History::default();
    let run = h.run();
    h.input(said("first"));
    let round = h.round(run);
    h.call(round, true);
    h.end_round(round, RoundEnd::ToolsSettled);
    let round = h.round(run);
    let early_reply = h.reply();
    h.end_round(round, RoundEnd::Replied);
    h.end_run(run, RunEnd::Completed);

    let (_, second) = h.answered("second");
    h.input(EventBody::Compact);
    let run = h.run();
    h.input(said("third"));
    let round = h.round(run);
    h.compact(second);
    h.reply();
    h.end_round(round, RoundEnd::Replied);
    h.end_run(run, RunEnd::Completed);

    // Reminders need no reply, so these Runs skip it while replies still go to the owner; the
    // second compacts its own first Round away.
    let reminder = || EventBody::Reminder {
        schedule: ScheduleId::new(),
        due_at: Utc::now(),
        skipped: 0,
        text: "check in".into(),
    };
    let run = h.run();
    h.input(reminder());
    let round = h.round(run);
    h.call(round, true);
    h.end_round(round, RoundEnd::ReplySkipped);
    h.end_run(run, RunEnd::Completed);
    let run = h.run();
    h.input(reminder());
    let round = h.round(run);
    h.call(round, true);
    let first = h.end_round(round, RoundEnd::ToolsSettled);
    let round = h.round(run);
    h.compact(first);
    h.call(round, true);
    h.end_round(round, RoundEnd::ReplySkipped);
    h.end_run(run, RunEnd::Completed);

    // A compaction within a Run, after the Run's first Round.
    let run = h.run();
    h.input(said("fourth"));
    let round = h.round(run);
    h.call(round, true);
    let first = h.end_round(round, RoundEnd::ToolsSettled);
    let round = h.round(run);
    h.compact(first);
    h.reply();
    h.end_round(round, RoundEnd::Replied);
    h.end_run(run, RunEnd::Completed);

    let run = h.run();
    h.input(said("fifth"));
    let round = h.round(run);
    h.refused(1_000);
    h.reply();
    h.end_round(round, RoundEnd::Replied);
    h.end_run(run, RunEnd::Completed);

    // Back to a reply before both compactions; a later compaction passes the backtrack.
    h.input(EventBody::Backtrack {
        to: Some(early_reply),
    });
    let (_, sixth) = h.answered("sixth");
    h.input(EventBody::Compact);
    let run = h.run();
    h.input(said("seventh"));
    let round = h.round(run);
    h.compact(sixth);
    h.reply();
    h.end_round(round, RoundEnd::Replied);
    h.end_run(run, RunEnd::Completed);

    // Back to the start, then to the first input after it, which the next Run answers again.
    h.input(EventBody::Backtrack { to: None });
    let (eighth, _) = h.answered("eighth");
    h.input(EventBody::Backtrack { to: Some(eighth) });
    let run = h.run();
    let round = h.round(run);
    h.reply();
    h.end_round(round, RoundEnd::Replied);
    h.end_run(run, RunEnd::Completed);

    let run = h.run();
    h.input(said("ninth"));
    let round = h.round(run);
    h.call(round, true);
    h.end_round(round, RoundEnd::ToolsSettled);
    h.end_run(run, RunEnd::Halted);
    h.input(halt());

    // The process stops while a call runs.
    let run = h.run();
    h.input(said("tenth"));
    let round = h.round(run);
    h.call(round, false);
    h.0
}
