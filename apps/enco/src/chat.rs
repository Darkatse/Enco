#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "this module is the terminal chat interface"
)]
use crate::{
    client::Client,
    paths::Paths,
    protocol::{Command, ServerMessage},
};
use anyhow::Result;
use enco_core::*;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, BufReader};

pub(crate) async fn chat(paths: &Paths, session: String) -> Result<()> {
    let mut client = Client::connect(&paths.socket()).await?;
    client
        .request(Command::Subscribe {
            session: session.clone(),
        })
        .await?;
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut purposes = HashMap::new();
    let mut safe_noted = false;
    loop {
        tokio::select! {
            line = input.next_line() => {
                let Some(line) = line? else { return Ok(()); };
                match line.as_str() {
                    "/quit" => return Ok(()),
                    "/cancel" => { client.send(Command::Cancel { session: session.clone() }).await?; }
                    "" => {},
                    _ => {
                        client.send(Command::Send {
                            session: session.clone(), text: line, event_id: EventId::new(),
                        }).await?;
                    }
                }
            }
            line = client.reader.next_line() => {
                let Some(line) = line? else { return Ok(()); };
                match serde_json::from_str::<ServerMessage>(&line)? {
                    ServerMessage::Entry { entry, .. } => render(*entry, &mut purposes, &mut safe_noted),
                    ServerMessage::Error { code, message, .. } => eprintln!("! {code}: {message}"),
                    ServerMessage::Ok { .. } => {},
                }
            }
        }
    }
}

fn render(entry: Entry, purposes: &mut HashMap<AttemptId, AttemptPurpose>, safe_noted: &mut bool) {
    match entry.body {
        EntryBody::RunStarted { .. } => *safe_noted = false,
        EntryBody::EventConsumed {
            event:
                Event {
                    body: EventBody::Reminder { text, .. },
                    ..
                },
        } => println!("⏰ {text}"),
        EntryBody::RoundStarted {
            safe_mode: true, ..
        } if !*safe_noted => {
            println!("(safe mode)");
            *safe_noted = true;
        }
        EntryBody::AttemptStarted {
            attempt, purpose, ..
        } => {
            purposes.insert(attempt, purpose);
        }
        EntryBody::AttemptSettled { attempt, result } => {
            let purpose = purposes.remove(&attempt);
            match result {
                AttemptResult::Completed { message, .. }
                    if purpose == Some(AttemptPurpose::Reply) =>
                {
                    let text = message.joined_text();
                    if !text.is_empty() {
                        println!("{text}");
                    }
                }
                AttemptResult::Failed { failure } => eprintln!(
                    "! model request failed: {}: {}",
                    failure.code, failure.message
                ),
                _ => {}
            }
        }
        EntryBody::ToolCallStarted { capability, .. } => println!("  → {}", capability.name),
        EntryBody::ToolCallSettled { outcome, .. } => match outcome {
            Settlement::Ok => println!("  ← ok"),
            Settlement::Failed { failure } => println!("  ← failed: {}", failure.code),
            Settlement::Unknown { failure } => println!("  ← unknown: {}", failure.code),
        },
        EntryBody::Compacted { .. } => println!("  (context compacted)"),
        EntryBody::RunEnded { end, .. } if end != RunEnd::Completed => {
            eprintln!("! run ended: {end:?}")
        }
        _ => {}
    }
}
