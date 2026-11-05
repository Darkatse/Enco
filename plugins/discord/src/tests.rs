use super::{
    Plugin,
    channel::{
        Arrival, Closure, Command, FileData, Guest, HttpRequest, HttpResponse, Identity, Outgoing,
        ReadStep, Reading,
    },
};
use enco_sdk::channel::{Action, CommandKind, Quote, Quoted};
use enco_sdk::channel::{ActivityUpdate, ActivityView, ToolActivity, ToolState};
use serde_json::{Value, json};

fn response(status: u16, body: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![],
        body: body.to_string().into_bytes(),
    }
}

fn identity() -> Result<Identity, String> {
    Plugin::identified(response(
        200,
        json!({"id":"app","bot":{"id":"42","username":"Helper"}}),
    ))
    .map_err(|error| error.failure.message)
}

fn arrival(state: Option<String>, arrival: Arrival) -> Result<Reading, String> {
    Plugin::received(identity()?, state, arrival).map_err(|error| error.failure.message)
}

fn frame(state: Option<String>, frame: Value) -> Result<Reading, String> {
    arrival(state, Arrival::Frame(frame.to_string()))
}

fn body(request: &HttpRequest) -> Result<Value, String> {
    serde_json::from_slice(request.body.as_deref().ok_or("missing body")?)
        .map_err(|error| error.to_string())
}

fn message() -> Value {
    json!({
        "id": "10",
        "channel_id": "thread",
        "guild_id": "guild",
        "type": 0,
        "author": {"id": "7", "username": "ada", "global_name": "Ada"},
        "content": "hello",
        "timestamp": "2026-10-29T12:00:00.000000+00:00",
        "mentions": [],
        "attachments": [],
        "embeds": [],
        "components": [],
    })
}

fn created(sequence: u64, message: Value) -> Result<Reading, String> {
    frame(
        None,
        json!({"op": 0, "s": sequence, "t": "MESSAGE_CREATE", "d": message}),
    )
}

#[test]
fn gateway_heartbeat_detects_missing_ack_despite_continuing_dispatches() -> Result<(), String> {
    let hello = frame(None, json!({"op":10,"d":{"heartbeat_interval":40_000}}))?;
    assert!(hello.timer.is_some_and(|delay| delay < 40_000));
    let identify: Value =
        serde_json::from_str(&hello.frames[0]).map_err(|error| error.to_string())?;
    assert_eq!(identify["op"], 2);
    assert!(identify["d"]["token"].is_null());
    assert_eq!(
        identify["d"]["intents"].as_u64().ok_or("intents")? & (1 << 15),
        1 << 15
    );
    let heartbeat = arrival(hello.state, Arrival::Timer)?;
    assert_eq!(heartbeat.timer, Some(40_000));
    assert_eq!(heartbeat.frames, [r#"{"d":null,"op":1}"#]);
    let dispatch = frame(
        heartbeat.state,
        json!({"op":0,"s":1,"t":"GUILD_CREATE","d":{}}),
    )?;
    assert!(dispatch.messages.is_empty());
    let dead = arrival(dispatch.state, Arrival::Timer)?;
    assert!(dead.reconnect);
    let ack = frame(heartbeat_state()?, json!({"op":11}))?;
    let live = arrival(ack.state, Arrival::Timer)?;
    assert!(!live.reconnect);
    Ok(())
}

fn heartbeat_state() -> Result<Option<String>, String> {
    let hello = frame(None, json!({"op":10,"d":{"heartbeat_interval":1000}}))?;
    Ok(arrival(hello.state, Arrival::Timer)?.state)
}

#[test]
fn ready_disconnect_resume_and_invalid_session_follow_protocol_state() -> Result<(), String> {
    let ready = json!({
        "session_id": "session",
        "resume_gateway_url": "wss://gateway-us.discord.gg/",
    });
    let ready = frame(None, json!({"op": 0, "s": 18, "t": "READY", "d": ready}))?;
    let closed = arrival(
        ready.state,
        Arrival::Closed(Closure {
            code: None,
            reason: "network disconnected".into(),
        }),
    )?;
    assert!(closed.reconnect);
    let ReadStep::Socket(socket) =
        Plugin::read(identity()?, closed.state.clone()).map_err(|error| error.failure.message)?
    else {
        return Err("expected socket".into());
    };
    assert_eq!(
        socket.url,
        "wss://gateway-us.discord.gg/?v=10&encoding=json"
    );
    assert_eq!(socket.close_code, 4000);
    let resumed = frame(
        closed.state,
        json!({"op":10,"d":{"heartbeat_interval":1000}}),
    )?;
    let resume: Value =
        serde_json::from_str(&resumed.frames[0]).map_err(|error| error.to_string())?;
    assert_eq!(
        resume,
        json!({"op":6,"d":{"token":null,"session_id":"session","seq":18}})
    );
    let invalid = frame(resumed.state, json!({"op":9,"d":false}))?;
    assert!(
        invalid
            .timer
            .is_some_and(|wait| (1_000..=5_000).contains(&wait))
    );
    assert!(arrival(invalid.state.clone(), Arrival::Timer)?.reconnect);
    let hello = frame(
        invalid.state,
        json!({"op":10,"d":{"heartbeat_interval":1000}}),
    )?;
    assert!(hello.frames[0].contains(r#""op":2"#));
    let identity = identity()?;
    let rejected = |code, reason: &str| {
        let closure = Closure {
            code: Some(code),
            reason: reason.into(),
        };
        Plugin::received(identity.clone(), None, Arrival::Closed(closure))
    };
    let Err(intent) = rejected(4014, "Disallowed intent") else {
        return Err("expected a rejected intent".into());
    };
    assert!(intent.fatal && intent.failure.message.contains("Message Content intent"));
    assert!(rejected(4004, "Authentication failed").is_err_and(|error| error.fatal));
    Ok(())
}

mod incoming;
mod outgoing;
mod quotes;

mod conversations;
