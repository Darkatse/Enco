use crate::{
    client::Client,
    paths::Paths,
    protocol::{Command, ServerMessage},
};
use anyhow::{Context, Result, bail};
use enco_core::{Entry, EntryBody, EventId, LogPos, RunEnd, RunId};
use serde_json::Value;

pub(crate) async fn wait(paths: &Paths, session: String, text: String) -> Result<Value> {
    let mut client = Client::connect(&paths.socket()).await?;
    client
        .request(Command::Subscribe {
            session: session.clone(),
        })
        .await?;
    let event_id = EventId::new();
    let send_id = client
        .send(Command::Send {
            session: session.clone(),
            text,
            event_id,
        })
        .await?;
    let mut accepted = None;
    let mut ended = None;
    let mut catch_up = None;
    let mut cursor = RunCursor {
        event: event_id,
        after: None,
        consumed: false,
    };
    loop {
        let line = client
            .reader
            .next_line()
            .await?
            .context("daemon closed the connection before the Run ended")?;
        let entries = match serde_json::from_str::<ServerMessage>(&line)? {
            ServerMessage::Ok { id, data } if id == send_id => {
                accepted = Some(data);
                Vec::new()
            }
            ServerMessage::Ok { id, data } if Some(id) == catch_up => {
                catch_up = None;
                serde_json::from_value(data)?
            }
            ServerMessage::Ok { id, .. } => bail!("unexpected reply to request {id}"),
            // The daemon writes one connection's replies and pushes in order, so the pending
            // catch-up already holds everything pushed before its reply.
            ServerMessage::Entry { .. } if catch_up.is_some() => Vec::new(),
            ServerMessage::Entry { entry, .. } => vec![*entry],
            ServerMessage::Error { code, .. } if code == "lagged" => {
                if catch_up.is_none() {
                    catch_up = Some(
                        client
                            .send(Command::Log {
                                session: session.clone(),
                                after: cursor.after,
                            })
                            .await?,
                    );
                }
                Vec::new()
            }
            ServerMessage::Error { code, message, .. } => bail!("{code}: {message}"),
        };
        for entry in entries {
            if ended.is_none() {
                ended = cursor.accept(entry);
            }
        }
        if let Some((run, end)) = &ended
            && let Some(mut accepted) = accepted.take()
        {
            accepted["run"] = serde_json::to_value(run)?;
            accepted["end"] = serde_json::to_value(end)?;
            return Ok(accepted);
        }
    }
}

/// Pushes after a catch-up may repeat entries in its reply; Log positions drop the overlap.
struct RunCursor {
    event: EventId,
    after: Option<LogPos>,
    consumed: bool,
}

impl RunCursor {
    fn accept(&mut self, entry: Entry) -> Option<(RunId, RunEnd)> {
        if self.after.is_some_and(|after| entry.pos <= after) {
            return None;
        }
        self.after = Some(entry.pos);
        match entry.body {
            EntryBody::EventConsumed { event } if event.id == self.event => self.consumed = true,
            EntryBody::RunEnded { run, end } if self.consumed => return Some((run, end)),
            _ => {}
        }
        None
    }
}
