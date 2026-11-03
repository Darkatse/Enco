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
        settings: ProviderSettings {
            base_url: "https://example.test".into(),
            model: model.into(),
            api_key_env: None,
            options: serde_json::json!({}),
        },
    }
}

pub(crate) fn completed(attempt: AttemptId, input_tokens: u64) -> EntryBody {
    EntryBody::AttemptSettled {
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
    EventBody::Halt(Halt {
        root: SessionRef {
            id: session_id(),
            name: "main".parse().unwrap(),
        },
        by: None,
    })
}
