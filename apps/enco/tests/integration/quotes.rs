#![expect(
    clippy::unwrap_used,
    reason = "fixture failures stop the scenario with their context"
)]
//! Factory mappers preserve quoted speech and its evidence through the consuming host boundaries.
use crate::{
    protocol::{Command, Output},
    support::{self, Daemon, channel},
};
use enco_core::{Entry, EntryBody, EventBody, MappingRecord, Part, QuoteKind, Quoted, RunEnd};
use enco_kernel::{ChannelAction, Inspection, Store};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, path},
};

const PIXEL: &[u8] = include_bytes!("fixtures/pixel.png");

async fn telegram_service(message: Value) -> MockServer {
    let server = MockServer::start().await;
    support::embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(support::response(
            json!({"role":"assistant","content":"Seen"}),
            "stop",
        ))
        .mount(&server)
        .await;
    Mock::given(path("/bot42:test-secret/getMe"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":{"id":42,"username":"FixtureBot"}})),
        )
        .mount(&server)
        .await;
    for method in [
        "setMyCommands",
        "sendChatAction",
        "editMessageText",
        "deleteMessage",
    ] {
        Mock::given(path(format!("/bot42:test-secret/{method}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":true})),
            )
            .mount(&server)
            .await;
    }
    for method in ["sendMessage", "sendRichMessage"] {
        Mock::given(path(format!("/bot42:test-secret/{method}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"ok":true,"result":{"message_id":900}})),
            )
            .mount(&server)
            .await;
    }
    Mock::given(path("/bot42:test-secret/getFile"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":{"file_path":"photos/pixel.png"}})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/file/bot42:test-secret/photos/pixel.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PIXEL))
        .mount(&server)
        .await;
    Mock::given(path("/bot42:test-secret/getUpdates"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":[]}))
                .set_delay(Duration::from_secs(60)),
        )
        .with_priority(10)
        .mount(&server)
        .await;
    Mock::given(path("/bot42:test-secret/getUpdates"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":[{"update_id":10,"message":message}]})),
        )
        .with_priority(1)
        .up_to_n_times(1)
        .mount(&server)
        .await;
    server
}

async fn telegram_run(message: Value) -> (Vec<Entry>, Inspection, Value) {
    let server = telegram_service(message).await;
    let mut document: Value =
        toml::from_str(&Daemon::config(&server, "openai-compatible")).unwrap();
    document["endpoint"]["chat"]["accepts"] = json!(["image"]);
    let mut daemon = Daemon::start_config(&toml::to_string(&document).unwrap()).await;
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: "main".into(),
        })
        .await
        .unwrap();
    // Register after subscribing, so the first input cannot race the observation boundary.
    document["channel"]["telegram"] = json!({"plugin":"telegram","base_url":server.uri(),"owner":"9","secrets":{"token":"ENCO_TEST_TELEGRAM_TOKEN"}});
    std::fs::write(
        daemon.root.path().join("config.toml"),
        toml::to_string(&document).unwrap(),
    )
    .unwrap();
    daemon
        .connect()
        .await
        .request(Command::Apply {
            output: Output::Json,
            reason: None,
        })
        .await
        .unwrap();
    let entries = support::finish(&mut client).await;
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    let inspection = serde_json::from_value(
        client
            .request(Command::Inspect {
                session: "main".into(),
                attempt_id: None,
            })
            .await
            .unwrap(),
    )
    .unwrap();
    let requests = server.received_requests().await.unwrap();
    let request = requests
        .iter()
        .find(|request| request.url.path() == "/chat/completions")
        .unwrap();
    let body = serde_json::from_slice(&request.body).unwrap();
    daemon.stop().await;
    (entries, inspection, body)
}

fn input(entries: &[Entry]) -> &EventBody {
    entries
        .iter()
        .find_map(|entry| match &entry.body {
            EntryBody::EventConsumed { event }
                if matches!(event.body, EventBody::UserMessage { .. }) =>
            {
                Some(&event.body)
            }
            _ => None,
        })
        .unwrap()
}

#[tokio::test]
async fn telegram_reply_images_reach_the_recorded_request() {
    let (entries, inspection, wire) = telegram_run(json!({
        "message_id":10,"date":1793275200,"chat":{"id":7,"type":"private"},
        "from":{"id":9,"is_bot":false,"first_name":"Ada"},"text":"@FixtureBot explain this",
        "entities":[{"type":"mention","offset":0,"length":11}],
        "reply_to_message":{"message_id":8,"date":1793275100,"chat":{"id":7,"type":"private"},
            "from":{"id":42,"is_bot":true,"first_name":"Fixture Bot"},"caption":"A quoted photo",
            "photo":[{"file_id":"photo","file_unique_id":"unique","width":1,"height":1}]}
    }))
    .await;
    let EventBody::UserMessage {
        text,
        images,
        quote: Some(quote),
        ..
    } = input(&entries)
    else {
        panic!("missing quote")
    };
    assert_eq!(text, "explain this");
    assert!(images.is_empty());
    assert_eq!(quote.kind, QuoteKind::Reply);
    assert!(quote.fetched.is_none());
    let Quoted::Messages(messages) = &quote.messages else {
        panic!("missing quoted message")
    };
    assert!(messages[0].yours && !messages[0].owner);
    assert_eq!(messages[0].text, "A quoted photo");
    let image = &messages[0].images[0];
    assert!(
        inspection
            .request
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .any(|part| matches!(part, Part::Image(recorded) if recorded == image))
    );
    let text: String = inspection
        .request
        .messages
        .iter()
        .map(|message| message.joined_text())
        .collect();
    assert!(text.contains("A quoted photo") && text.contains("replies"));
    assert!(
        wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .any(|part| part["type"] == "image_url"
                && part["image_url"]["url"]
                    .as_str()
                    .unwrap()
                    .starts_with("data:image/png;base64,"))
    );
}

#[tokio::test]
async fn telegram_pure_forward_is_input_even_when_its_original_text_is_a_command() {
    let (entries, inspection, _) = telegram_run(json!({
        "message_id":10,"date":1793275200,"chat":{"id":7,"type":"private"},
        "from":{"id":9,"is_bot":false,"first_name":"Ada"},"text":"/cancel",
        "entities":[{"type":"bot_command","offset":0,"length":7}],
        "forward_origin":{"type":"channel","date":1793188800,"chat":{"id":-100,"type":"channel","title":"Trip News"},"message_id":2}
    })).await;
    let EventBody::UserMessage {
        text,
        quote: Some(quote),
        ..
    } = input(&entries)
    else {
        panic!("missing forward input")
    };
    assert!(text.is_empty());
    assert_eq!(quote.kind, QuoteKind::Forward);
    let Quoted::Messages(messages) = &quote.messages else {
        panic!("missing forwarded message")
    };
    assert_eq!(messages[0].text, "/cancel");
    assert_eq!(messages[0].conversation_name.as_deref(), Some("Trip News"));
    let text: String = inspection
        .request
        .messages
        .iter()
        .map(|message| message.joined_text())
        .collect();
    assert!(text.contains("forwards") && text.contains("Trip News") && text.contains("/cancel"));
}

#[tokio::test]
async fn onebot_reply_to_forward_records_the_last_fetch_with_both_responses() {
    if channel::child("quotes::onebot_reply_to_forward_records_the_last_fetch_with_both_responses")
    {
        return;
    }
    let server = MockServer::start().await;
    Mock::given(path("/get_login_info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"status":"ok","retcode":0,"data":{"user_id":42,"nickname":"Fixture"}}),
        ))
        .mount(&server)
        .await;
    let (_root, store, adapter) = channel::open(
        include_bytes!(env!("ENCO_FACTORY_ONEBOT")),
        "access_token",
        &server.uri(),
    )
    .await;
    let reply = json!({"status":"ok","retcode":0,"data":{
        "message_id":-7,"user_id":9,"sender":{"user_id":9,"nickname":"Ada","card":""},"time":1793275200,
        "message":[{"type":"forward","data":{"id":"trip-forward"}}]
    }});
    let forward = json!({"status":"ok","retcode":0,"data":{"messages":[{
        "message_id":0,"user_id":80,"sender":{"user_id":80,"nickname":"Bob","card":"Trip Bob"},"time":1793188800,
        "message":[{"type":"text","data":{"text":"meet at eight"}},{"type":"image","data":{"summary":"broken image"}}]
    }]}});
    for (route, arguments, response) in [
        ("get_msg", json!({"message_id":-7}), &reply),
        ("get_forward_msg", json!({"id":"trip-forward"}), &forward),
    ] {
        Mock::given(path(format!("/{route}")))
            .and(body_json(arguments))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;
    }
    let fetched = adapter.fetch_quote("onebot:reply:-7").await.unwrap();
    let (messages, origin) = (fetched.content.unwrap(), fetched.origin.unwrap());
    assert_eq!(messages[0].sender.as_deref(), Some("80"));
    assert_eq!(messages[0].sender_name.as_deref(), Some("Trip Bob"));
    assert_eq!(messages[0].text, "meet at eight");
    assert_eq!(messages[0].unviewable.len(), 1);
    let record: MappingRecord =
        serde_json::from_slice(&store.get_blob(&origin.call).await.unwrap()).unwrap();
    assert_eq!(record.export, "channel.fetch");
    assert_eq!(record.arguments["locator"], "onebot:reply:-7");
    let responses = record.arguments["responses"].as_array().unwrap();
    assert_eq!(responses.len(), 2);
    for (recorded, expected) in responses.iter().zip([reply, forward]) {
        let hash = serde_json::from_value(recorded["body"].clone()).unwrap();
        let bytes = store.get_blob(&hash).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), expected);
    }
}

#[tokio::test]
async fn discord_resumed_forward_frames_keep_their_quote_at_the_host_receiver() {
    if channel::child(
        "quotes::discord_resumed_forward_frames_keep_their_quote_at_the_host_receiver",
    ) {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let serving = tokio::spawn(async move {
        // The selected API service also accepts the resumed WebSocket, as in the transport contract.
        let (mut http, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(http.read_u8().await.unwrap());
        }
        assert!(
            String::from_utf8(headers)
                .unwrap()
                .starts_with("GET /applications/@me ")
        );
        let body =
            json!({"id":"app","bot":{"id":"42","username":"Fixture","bot":true}}).to_string();
        http.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        http.shutdown().await.unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        socket.send(Message::text(json!({"op":0,"s":2,"t":"MESSAGE_CREATE","d":{
            "id":"10","channel_id":"7","type":0,"author":{"id":"9","username":"ada","global_name":"Ada"},
            "content":"","timestamp":"2026-10-29T12:00:00Z","mentions":[],"attachments":[],
            "message_reference":{"type":1,"message_id":"8","channel_id":"elsewhere"},
            "message_snapshots":[{"message":{"type":0,"content":"Hello <:wave:123>","timestamp":"2026-10-28T12:00:00Z","mentions":[],"attachments":[]}}]
        }}).to_string())).await.unwrap();
        assert!(matches!(
            socket.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        socket.flush().await.unwrap();
    });
    let (_root, store, adapter) = channel::open(
        include_bytes!(env!("ENCO_FACTORY_DISCORD")),
        "token",
        &format!("http://{address}"),
    )
    .await;
    let protocol = json!({"session":"resumed-session","resume_url":format!("ws://{address}"),"sequence":1,"heartbeat_interval":null,"awaiting_ack":false});
    let enco_kernel::ChannelReading::Batch { messages, .. } =
        channel::reading(adapter, Some(protocol)).await.unwrap()
    else {
        panic!("missing batch")
    };
    let ChannelAction::Message(content) = &messages[0].input.action else {
        panic!("missing message")
    };
    assert!(messages[0].input.direct && content.text.is_empty());
    let quote = content.quote.as_ref().unwrap();
    assert_eq!(quote.kind, QuoteKind::Forward);
    let enco_kernel::ChannelQuoted::Messages(quoted) = &quote.messages else {
        panic!("missing snapshot")
    };
    assert_eq!(quoted[0].text, "Hello :wave:");
    assert!(quoted[0].sender.is_none());
    let record: MappingRecord =
        serde_json::from_slice(&store.get_blob(&messages[0].origin.call).await.unwrap()).unwrap();
    assert_eq!(record.export, "channel.received");
    assert!(
        record.arguments["arrival"]["frame"]
            .as_str()
            .unwrap()
            .contains("message_snapshots")
    );
    serving.await.unwrap();
}
