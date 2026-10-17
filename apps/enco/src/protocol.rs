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
    Status {
        output: Output,
    },
    Sessions {},
    Log {
        session: String,
        after: Option<LogPos>,
    },
    Apply {
        output: Output,
        reason: Option<Reason>,
    },
    History {
        output: Output,
        query: enco_host::development::HistoryQuery,
    },
    Rewind {
        output: Output,
        arguments: enco_host::development::RewindArguments,
    },
    Rollback {},
    PluginDeploy {
        output: Output,
        name: PluginName,
        path: std::path::PathBuf,
        reason: Option<Reason>,
    },
    PluginScaffold {
        name: PluginName,
    },
    PluginRename {
        output: Output,
        name: PluginName,
        to: PluginName,
        reason: Option<Reason>,
    },
    Inspect {
        session: String,
        attempt_id: Option<AttemptId>,
    },
    SetProfile {
        session: String,
        profile: String,
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
        entry: Box<Entry>,
    },
}

/// Rendering happens beside the facts; clients choose the wire representation.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Output {
    Json,
    Document,
}
