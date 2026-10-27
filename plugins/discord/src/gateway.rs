//! A replayable Gateway state machine, including resumable sequence and heartbeat state.
use crate::{
    channel::{Arrival, Fault, Identity, ReadStep, Reading, Socket},
    incoming, wire,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const GATEWAY: &str = "wss://gateway.discord.gg/";
const GUILDS: u32 = 1 << 0;
const GUILD_MESSAGES: u32 = 1 << 9;
const DIRECT_MESSAGES: u32 = 1 << 12;
const MESSAGE_CONTENT: u32 = 1 << 15;
/// The events Identify asks for: guilds, guild and direct messages, and the messages' content.
const INTENTS: u32 = GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT;
/// How long to wait after an Invalid Session before connecting again; Discord asks for 1 to 5
/// seconds.
const INVALID_SESSION_WAIT_MS: u32 = 3_000;

#[derive(Default, Deserialize, Serialize)]
struct State {
    session: Option<String>,
    resume_url: Option<String>,
    sequence: Option<u64>,
    heartbeat_interval: Option<u32>,
    awaiting_ack: bool,
}

impl State {
    fn parse(state: Option<&str>) -> Result<Self, Fault> {
        state.map_or(Ok(Self::default()), |state| {
            serde_json::from_str(state).map_err(|_| {
                wire::fault(
                    "channel.invalid_state",
                    "invalid Discord Gateway state",
                    true,
                )
            })
        })
    }
}

#[derive(Deserialize)]
struct Frame {
    op: u8,
    #[serde(default)]
    d: Value,
    s: Option<u64>,
    t: Option<String>,
}

pub(super) fn read(state: Option<&str>) -> Result<ReadStep, Fault> {
    let state = State::parse(state)?;
    Ok(ReadStep::Socket(Socket {
        url: format!(
            "{}/?v=10&encoding=json",
            state
                .resume_url
                .as_deref()
                .unwrap_or(GATEWAY)
                .trim_end_matches('/')
        ),
        // Normal closure invalidates a Discord session instead of preserving it for Resume.
        close_code: 4000,
    }))
}

pub(super) fn received(
    identity: Identity,
    state: Option<&str>,
    arrival: Arrival,
) -> Result<Reading, Fault> {
    let mut state = State::parse(state)?;
    let mut reading = Reading {
        messages: vec![],
        state: None,
        frames: vec![],
        acknowledgments: vec![],
        timer: None,
        reconnect: false,
    };
    match arrival {
        Arrival::Frame(frame) => frame_received(&identity, &mut state, &mut reading, &frame)?,
        Arrival::Timer => {
            if let Some(interval) = state.heartbeat_interval {
                if state.awaiting_ack {
                    reading.reconnect = true;
                } else {
                    heartbeat(&mut state, &mut reading);
                    reading.timer = Some(interval);
                }
            } else {
                // Without a heartbeat interval, this timer was the wait after an Invalid Session.
                reading.reconnect = true;
            }
        }
        Arrival::Closed(closure) => {
            if let Some(code @ (4004 | 4010..=4014)) = closure.code {
                let reason = if code == 4014 {
                    "enable Message Content intent in the Discord Developer Portal"
                } else {
                    "check bot token, intents and sharding configuration"
                };
                return Err(wire::fault(
                    "channel.gateway_rejected",
                    format!(
                        "Discord Gateway closed with {code}: {}; {reason}",
                        closure.reason
                    ),
                    true,
                ));
            }
            if matches!(closure.code, Some(4007 | 4009)) {
                state = State::default();
            }
            reading.reconnect = true;
        }
        Arrival::Response(_) => return Err(wire::invalid("Gateway arrival")),
    }
    reading.state =
        Some(serde_json::to_string(&state).map_err(|error| wire::invalid(&error.to_string()))?);
    Ok(reading)
}

fn heartbeat(state: &mut State, reading: &mut Reading) {
    reading
        .frames
        .push(json!({"op": 1, "d": state.sequence}).to_string());
    state.awaiting_ack = true;
}

fn frame_received(
    identity: &Identity,
    state: &mut State,
    reading: &mut Reading,
    raw: &str,
) -> Result<(), Fault> {
    let frame: Frame = serde_json::from_str(raw).map_err(|_| wire::invalid("Gateway frame"))?;
    match frame.op {
        0 => {
            state.sequence = Some(frame.s.ok_or_else(|| wire::invalid("dispatch sequence"))?);
            match frame
                .t
                .as_deref()
                .ok_or_else(|| wire::invalid("dispatch type"))?
            {
                "READY" => {
                    #[derive(Deserialize)]
                    struct Ready {
                        session_id: String,
                        resume_gateway_url: String,
                    }
                    let ready: Ready =
                        serde_json::from_value(frame.d).map_err(|_| wire::invalid("READY"))?;
                    state.session = Some(ready.session_id);
                    state.resume_url = Some(ready.resume_gateway_url);
                }
                "MESSAGE_CREATE" => {
                    if let Some(message) = incoming::message(frame.d, &identity.account)? {
                        reading.messages.push(message);
                    }
                }
                "INTERACTION_CREATE" => {
                    if let Some((message, acknowledgment)) = incoming::interaction(frame.d)? {
                        reading.messages.push(message);
                        reading.acknowledgments.push(acknowledgment);
                    }
                }
                _ => {}
            }
        }
        1 => heartbeat(state, reading),
        7 => reading.reconnect = true,
        9 => {
            if !frame
                .d
                .as_bool()
                .ok_or_else(|| wire::invalid("Invalid Session"))?
            {
                *state = State::default();
            }
            state.heartbeat_interval = None;
            reading.timer = Some(INVALID_SESSION_WAIT_MS);
        }
        10 => {
            #[derive(Deserialize)]
            struct Hello {
                heartbeat_interval: u32,
            }
            let hello: Hello =
                serde_json::from_value(frame.d).map_err(|_| wire::invalid("Hello"))?;
            if hello.heartbeat_interval == 0 {
                return Err(wire::invalid("heartbeat interval"));
            }
            state.heartbeat_interval = Some(hello.heartbeat_interval);
            state.awaiting_ack = false;
            // Discord asks for a random first beat within the interval; the mapper has no
            // randomness, so the account decides it.
            let jitter = identity.account.bytes().fold(0u32, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(u32::from(byte))
            });
            reading.timer = Some(jitter % hello.heartbeat_interval);
            let authenticate = match (&state.session, state.sequence) {
                (Some(session), Some(sequence)) => {
                    let resume = json!({"token": null, "session_id": session, "seq": sequence});
                    json!({"op": 6, "d": resume})
                }
                _ => {
                    let properties = json!({"os": "enco", "browser": "enco", "device": "enco"});
                    let identify = json!({
                        "token": null,
                        "intents": INTENTS,
                        "properties": properties,
                    });
                    json!({"op": 2, "d": identify})
                }
            };
            reading.frames.push(authenticate.to_string());
        }
        11 => state.awaiting_ack = false,
        _ => {}
    }
    Ok(())
}
