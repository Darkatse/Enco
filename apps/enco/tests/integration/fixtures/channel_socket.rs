//! A pure test protocol whose network origins are fixed before its generation loads.
use bindings::exports::enco::plugin::{channel, lifecycle};
use enco_sdk::{types, channel::{Action, Content}};
use serde_json::{Value, json};

enco_sdk::export!(channel, Fixture);
struct Fixture;
const SOCKET: &str = "SOCKET_ORIGIN";
const CDN: &str = "CDN_ORIGIN";

fn request(method: &str, url: &str) -> channel::Request {
    channel::Request { method: method.into(), url: url.into(), headers: Vec::new(), body: None, timeout_ms: None }
}

fn reading() -> channel::Reading {
    channel::Reading { messages: Vec::new(), state: None, frames: Vec::new(), acknowledgments: Vec::new(), timer: None, reconnect: false }
}

impl lifecycle::Guest for Fixture {
    fn describe(_config: String) -> Result<lifecycle::Description, types::Failure> {
        Ok(lifecycle::Description {
            summary: "A deterministic socket fixture".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential { name: "token".into(), required: true, placements: vec![
                lifecycle::Placement { origin: None, location: lifecycle::Location::Header(lifecycle::HeaderPlacement { name: "authorization".into(), prefix: "Bot ".into() }) },
                lifecycle::Placement { origin: Some(SOCKET.into()), location: lifecycle::Location::Frame("/token".into()) },
            ] }],
        })
    }
    async fn probe() -> Result<(), types::Failure> { Ok(()) }
}

impl channel::Guest for Fixture {
    fn pacing() -> Option<channel::PacingGap> { None }
    fn origins() -> Vec<String> { vec![SOCKET.into(), CDN.into()] }
    fn identify() -> channel::Request { request("GET", "/identify") }
    fn identified(response: channel::Response) -> Result<channel::Identity, channel::Fault> {
        Ok(channel::Identity { protocol: "fixture".into(), account: "one".into(), facts: String::from_utf8(response.body).unwrap() })
    }
    fn read(_identity: channel::Identity, state: Option<String>) -> Result<channel::ReadStep, channel::Fault> {
        Ok(channel::ReadStep::Socket(channel::Socket { url: format!("{SOCKET}/?resume={}", state.as_deref().unwrap_or("null")), close_code: 4000 }))
    }
    fn received(_identity: channel::Identity, _state: Option<String>, arrival: channel::Arrival) -> Result<channel::Reading, channel::Fault> {
        let mut result = reading();
        match arrival {
            channel::Arrival::Frame(frame) => {
                let value: Value = serde_json::from_str(&frame).unwrap();
                let seq = value["seq"].as_u64().unwrap();
                result.state = Some(seq.to_string());
                result.messages.push(channel::Inbound {
                    key: seq.to_string(), conversation: "one".into(), message: seq.to_string(), sender: Some("owner".into()), sender_name: None,
                    direct: true, addressed: true, reply_to: None, respond: Some(seq.to_string()),
                    action: Action::Message(Content { text: frame, images: Vec::new(), unviewable: Vec::new() }),
                });
                if seq == 1 {
                    result.frames.push(json!({"op":"identify","token":null}).to_string());
                    result.acknowledgments.push(request("POST", "/ack"));
                    result.timer = Some(1);
                }
            }
            channel::Arrival::Timer => result.frames.push(json!({"op":"heartbeat"}).to_string()),
            channel::Arrival::Closed(_) => result.reconnect = true,
            channel::Arrival::Response(_) => panic!("socket fixture does not read requests"),
        }
        Ok(result)
    }
    fn fetch(locator: String, responses: Vec<channel::Response>) -> Result<channel::FetchStep, channel::Fault> {
        Ok(match responses.last() {
            Some(response) if locator != "endless" => channel::FetchStep::Image(response.body.clone()),
            _ => channel::FetchStep::Download(format!("{CDN}/file")),
        })
    }
    fn split(message: channel::Outgoing) -> Vec<channel::Outgoing> { vec![message] }
    fn send(_conversation: String, _part: channel::Outgoing) -> Result<channel::Request, channel::Fault> { Ok(request("POST", "/send")) }
    fn answered(_response: channel::Response) -> Result<Option<String>, channel::Fault> { Ok(None) }
    fn respond(_handle: String, _text: Option<String>) -> Result<Option<channel::Request>, channel::Fault> { Ok(None) }
    fn commands(_identity: channel::Identity, _commands: Vec<channel::Command>) -> Vec<channel::Request> { Vec::new() }
    fn activity(_conversation: String, _shown: Option<String>, _activity: Option<enco_sdk::channel::ActivityView>) -> Option<channel::Request> { None }
}
