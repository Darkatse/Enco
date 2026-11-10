//! Version 23 documents in their version 24 form.
//!
//! Code is named by registration or as factory code, the rollback notice is the plugin's, and a
//! delegation names its target: an existing child, or the child it created with its creation
//! record. Mapping records move to new addresses, and the origins that point at them follow.
//!
//! What changed form is rewritten as JSON, keyed by the documents' own tags; origins are found by
//! type after decoding, so content that plugins and services own is never read.
use super::registry::Registry;
use crate::store::{backend, evidence};
use enco_core::*;
use enco_kernel::StoreError;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeMap;

/// What the JSON rewrites read: the old registry and the Sessions as version 23 left them.
pub(super) struct Rewrite<'a> {
    pub registry: &'a Registry,
    pub sessions: &'a BTreeMap<SessionId, SessionRecord>,
}

fn invalid(what: &str, value: &Value) -> StoreError {
    backend(format!("unexpected {what}: {value}"))
}

fn number(value: &Value) -> Result<u64, StoreError> {
    value
        .as_u64()
        .ok_or_else(|| invalid("generation number", value))
}

fn json(value: &impl serde::Serialize) -> Result<Value, StoreError> {
    serde_json::to_value(value).map_err(backend)
}

/// The creation record of a Session, with the values it has now: version 23 did not record
/// them when it was created.
pub(super) fn creation(row: &SessionRecord) -> Creation {
    Creation {
        id: row.id,
        name: row.name.clone(),
        parent: row.parent.clone(),
        created_at: row.created_at,
        node: row.binding.node,
        profile: row.profile.clone(),
        working_dir: row.working_dir.clone(),
    }
}

impl Rewrite<'_> {
    /// A Log entry body.
    pub fn entry(&self, body: &mut Value) -> Result<(), StoreError> {
        match body["kind"].as_str() {
            Some("attempt_started") => {
                for key in ["composer", "provider"] {
                    body[key] = json(&self.registry.code(number(&body[key])?)?)?;
                }
            }
            Some("tool_call_started") if body["code"]["kind"] == "generation" => {
                let code = self.registry.code(number(&body["code"]["id"])?)?;
                body["code"] = json(&ToolCode::Plugin(code))?;
            }
            Some("round_ended") if body["end"]["kind"] == "failed" => {
                let end = body["end"]
                    .as_object_mut()
                    .ok_or_else(|| backend("a failed Round end is not an object"))?;
                let generations = end
                    .remove("generations")
                    .ok_or_else(|| backend("a failed Round names no active generations"))?;
                let ids = generations
                    .as_array()
                    .ok_or_else(|| invalid("active generations", &generations))?
                    .iter()
                    .map(number)
                    .collect::<Result<Vec<_>, _>>()?;
                end.insert("active".into(), json(&self.registry.active(&ids)?)?);
            }
            Some("event_consumed") => self.event(&mut body["event"])?,
            Some("tool_call_settled") => self.delegated(body)?,
            _ => {}
        }
        Ok(())
    }

    /// An Event, in the Inbox or consumed: the rollback notice is the plugin's.
    pub fn event(&self, event: &mut Value) -> Result<(), StoreError> {
        let body = &mut event["body"];
        if body["kind"] != "generation_rolled_back" {
            return Ok(());
        }
        body["kind"] = "plugin_rolled_back".into();
        let rollback = &mut body["rollback"];
        let from = self.registry.generation(number(&rollback["from"])?)?;
        rollback["from"] = json(&from)?;
        rollback["to"] = match &rollback["to"] {
            Value::Null => Value::Null,
            to => json(&self.registry.plugin_code(from.plugin, number(to)?)?)?,
        };
        Ok(())
    }

    /// A delegation names its child: an existing one, or the one it created, by its creation.
    fn delegated(&self, body: &mut Value) -> Result<(), StoreError> {
        let Some(delegated) = body.get_mut("delegated").and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let created = delegated
            .remove("created")
            .and_then(|created| created.as_bool())
            .ok_or_else(|| backend("a delegation does not say whether it created its child"))?;
        let child: SessionRef = serde_json::from_value(
            delegated
                .remove("session")
                .ok_or_else(|| backend("a delegation names no child"))?,
        )
        .map_err(backend)?;
        let target = if created {
            let row = self
                .sessions
                .get(&child.id)
                .ok_or(StoreError::UnknownSession(child.id))?;
            DelegationTarget::Created(creation(row))
        } else {
            DelegationTarget::Existing(child)
        };
        delegated.insert("target".into(), json(&target)?);
        Ok(())
    }
}

/// Mapping records in their version 24 form, moved as the origins that point at them are met.
///
/// Only a record some origin points at moves, and it must be a version 23 record; no mapper's
/// arguments hold an origin, so records do not point at each other. Other evidence, such as the
/// responses records read, is left as it is.
pub(super) struct Records<'a> {
    connection: &'a Connection,
    registry: &'a Registry,
    /// Each moved record's new address, by its old one.
    moved: BTreeMap<ContentHash, ContentHash>,
}

impl<'a> Records<'a> {
    pub fn new(connection: &'a Connection, registry: &'a Registry) -> Self {
        Self {
            connection,
            registry,
            moved: BTreeMap::new(),
        }
    }

    /// Point `origins` at their records' new addresses; whether there were any.
    pub fn follow<'o>(
        &mut self,
        origins: impl IntoIterator<Item = &'o mut MappingOrigin>,
    ) -> Result<bool, StoreError> {
        let mut followed = false;
        for origin in origins {
            origin.call = match self.moved.get(&origin.call) {
                Some(new) => *new,
                None => {
                    let new = self.move_record(origin.call)?;
                    self.moved.insert(origin.call, new);
                    new
                }
            };
            followed = true;
        }
        Ok(followed)
    }

    /// Store the record at `old` in the version 24 form, which names its code instead of a
    /// generation number, and remove the old form.
    fn move_record(&self, old: ContentHash) -> Result<ContentHash, StoreError> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Recorded {
            generation: u64,
            export: String,
            arguments: Value,
        }
        let recorded: Recorded = serde_json::from_slice(&evidence::get(self.connection, old)?)
            .map_err(|error| backend(format!("mapping record {old}: {error}")))?;
        let record = MappingRecord {
            code: self.registry.code(recorded.generation)?,
            export: recorded.export,
            arguments: recorded.arguments,
        };
        let new = evidence::put(
            self.connection,
            &serde_json::to_vec(&record).map_err(backend)?,
        )?;
        self.connection
            .execute("DELETE FROM evidence WHERE hash=?", [old.to_string()])
            .map_err(backend)?;
        Ok(new)
    }
}

/// The origins an Event holds: the channel input's, its earlier messages' and its quote's fetch.
pub(super) fn event_origins(event: &mut Event) -> Vec<&mut MappingOrigin> {
    let mut origins = Vec::new();
    if let EventSource::Channel { origin, .. } = &mut event.source {
        origins.push(origin);
    }
    if let EventBody::UserMessage { earlier, quote, .. } = &mut event.body {
        origins.extend(earlier.iter_mut().map(|message| &mut message.origin));
        origins.extend(quote.as_mut().and_then(|quote| quote.fetched.as_mut()));
    }
    origins
}

/// The origins a Log entry holds: its consumed Event's and the decoded response's.
pub(super) fn entry_origins(body: &mut EntryBody) -> Vec<&mut MappingOrigin> {
    match body {
        EntryBody::EventConsumed { event } => event_origins(event),
        EntryBody::AttemptSettled { origin, .. } => origin.iter_mut().collect(),
        _ => Vec::new(),
    }
}

/// Rewrite the Inbox, the deliveries and the connections' states in place; each must decode as
/// its current type.
pub(super) fn rewrite_tables(
    connection: &Connection,
    rewrite: &Rewrite<'_>,
    records: &mut Records<'_>,
) -> Result<(), StoreError> {
    rewrite_column::<Event>(
        connection,
        ["inbox", "order_no", "event"],
        |event| rewrite.event(event),
        |event| records.follow(event_origins(event)),
    )?;
    rewrite_column::<DeliverySettlement>(
        connection,
        ["deliveries", "order_no", "body"],
        |_| Ok(()),
        |settlement| records.follow(&mut settlement.origins),
    )?;
    // A connection's state is the host's own record of its conversations, which quotes earlier
    // messages with their origins.
    rewrite_column::<crate::channel::State>(
        connection,
        ["connections", "key", "state"],
        |_| Ok(()),
        |state| records.follow(state.origins_mut()),
    )
}

/// Rewrite one JSON column: what changed form as JSON, then the decoded value; writes back only
/// what changed.
fn rewrite_column<T: serde::de::DeserializeOwned + serde::Serialize>(
    connection: &Connection,
    [table, key, column]: [&str; 3],
    mut form: impl FnMut(&mut Value) -> Result<(), StoreError>,
    mut typed: impl FnMut(&mut T) -> Result<bool, StoreError>,
) -> Result<(), StoreError> {
    let rows = connection
        .prepare(&format!(
            "SELECT {key},{column} FROM {table} ORDER BY {key}"
        ))
        .map_err(backend)?
        .query_map([], |row| {
            Ok((
                row.get::<_, rusqlite::types::Value>(0)?,
                row.get::<_, String>(1)?,
            ))
        })
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    for (id, document) in rows {
        let original: Value = serde_json::from_str(&document).map_err(backend)?;
        let mut value = original.clone();
        form(&mut value)?;
        let mut decoded: T = serde_json::from_value(value.clone())
            .map_err(|error| backend(format!("{table} {id:?}: {error}")))?;
        if typed(&mut decoded)? || value != original {
            connection
                .execute(
                    &format!("UPDATE {table} SET {column}=? WHERE {key}=?"),
                    rusqlite::params![crate::store::encode(&decoded)?, id],
                )
                .map_err(backend)?;
        }
    }
    Ok(())
}
