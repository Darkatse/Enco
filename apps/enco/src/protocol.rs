use enco_core::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Send {
        session: String,
        text: String,
        event_id: EventId,
    },
    Subscribe {
        session: String,
    },
    Cancel {
        session: String,
    },
    // Empty struct variants retain deny_unknown_fields for argument-free commands.
    Schedules {},
    CancelSchedule {
        schedule_id: ScheduleId,
    },
    Memories {},
    ForgetMemory {
        memory_id: MemoryId,
    },
    Status {},
    Sessions {},
    Log {
        session: String,
        after: Option<LogPos>,
    },
    SafeMode {
        enabled: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ServerMessage {
    Ok {
        id: u64,
        data: serde_json::Value,
    },
    Error {
        id: u64,
        code: String,
        message: String,
    },
    Entry {
        session: String,
        entry: Entry,
    },
}
