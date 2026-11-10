//! The subset of Enco's public CLI output used by the evaluator.
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct Status {
    pub(crate) node: Node,
    pub(crate) plugins: std::collections::BTreeMap<String, PluginUnit>,
}

#[derive(Deserialize)]
pub(crate) struct Node {
    pub(crate) sessions: Vec<SessionStatus>,
}

#[derive(Deserialize)]
pub(crate) struct PluginUnit {
    pub(crate) running: Option<Plugin>,
}

#[derive(Deserialize)]
pub(crate) struct SessionStatus {
    pub(crate) session: Session,
    pub(crate) running: bool,
}

#[derive(Deserialize)]
pub(crate) struct Session {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct Plugin {
    pub(crate) running: Running,
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum Running {
    Serving {
        name: String,
        activation: Activation,
    },
    Unavailable {
        name: String,
        activation: Activation,
    },
    Stopped,
}

#[derive(Deserialize)]
pub(crate) struct Activation {
    pub(crate) code: CodeRef,
}

#[derive(Deserialize)]
pub(crate) struct Entry {
    pub(crate) body: EntryBody,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum EntryBody {
    EventConsumed {
        event: Event,
    },
    RoundStarted,
    AttemptStarted {
        attempt: String,
        purpose: Purpose,
        round: String,
    },
    AttemptSettled {
        attempt: String,
        result: AttemptResult,
    },
    ToolCallStarted {
        call: String,
        code: CodeRef,
    },
    ToolCallSettled {
        call: String,
        outcome: Settlement,
        content: String,
        outgoing: Option<Outgoing>,
    },
    RunEnded {
        end: RunEnd,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CodeRef {
    Generation {
        id: GenerationId,
    },
    Factory {},
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(crate) struct GenerationId {
    registration: u64,
}

impl CodeRef {
    /// Plugin code as Enco's text names it, `registration 15` or `factory code`; native code has
    /// no such name.
    pub(crate) fn name(&self) -> Option<String> {
        match self {
            Self::Generation { id } => Some(format!("registration {}", id.registration)),
            Self::Factory {} => Some("factory code".into()),
            Self::Other => None,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct Outgoing {
    pub(crate) text: String,
    pub(crate) file: Option<OutgoingFile>,
}

#[derive(Deserialize)]
pub(crate) struct OutgoingFile {
    pub(crate) name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Purpose {
    Reply,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(crate) struct Event {
    pub(crate) body: EventBody,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum EventBody {
    UserMessage {
        text: String,
    },
    Reminder {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AttemptResult {
    Completed { message: Message, usage: Usage },
    Failed,
    Overflow,
}

#[derive(Deserialize)]
pub(crate) struct Message {
    pub(crate) parts: Vec<Part>,
}

#[derive(Deserialize)]
pub(crate) struct Usage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cached_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Part {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Settlement {
    Ok,
    Failed { failure: Failure },
    Unknown { failure: Failure },
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RunEnd {
    Completed,
    BudgetExhausted,
    Interrupted,
    Cancelled,
    Backtracked,
    Halted,
    Failed { failure: Failure },
}
