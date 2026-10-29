#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "fixture failures retain their context"
)]
//! The factory OneBot mapper uses the selected service for both HTTP and authenticated WebSocket.
use crate::support::channel;
use enco_core::{FileData, Outgoing};
use enco_kernel::{AttachmentContent, ChannelAction, ChannelAdapter, ChannelReading};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::derive_accept_key, protocol::Role},
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn onebot_receives_and_sends_through_the_shared_host_contract() {
    if channel::child("onebot::onebot_receives_and_sends_through_the_shared_host_contract") {
        return;
    }
    let mut server = Server::start().await;
    let (_root, _store, adapter) = channel::open(
        include_bytes!(env!("ENCO_FACTORY_ONEBOT")),
        "access_token",
        &server.url,
    )
    .await;
    assert_eq!(adapter.identity().key(), "onebot:42");
    assert!(adapter.pacing().is_some());
    // An image link from a message may point anywhere. Outside the mapper's origins, the API
    // included, it leaves only that image unviewable; the server fails on any request it gets.
    for link in [
        "https://images.example.com/a.png".to_owned(),
        format!("{}/send_private_msg?user_id=9&message=x", server.url),
    ] {
        assert!(
            matches!(
                adapter.fetch(&link).await.unwrap(),
                AttachmentContent::Unavailable(_)
            ),
            "{link}"
        );
    }
    receive_inputs(adapter.clone()).await;
    send_outputs(adapter.as_ref(), &mut server.requests).await;
    server.stop.cancel();
    server.task.await.unwrap();
}

async fn receive_inputs(adapter: Arc<dyn ChannelAdapter>) {
    let (send, mut readings) = mpsc::unbounded_channel();
    let stop = CancellationToken::new();
    let receiving = adapter.clone();
    let cancelled = stop.clone();
    let receiver = tokio::spawn(async move { receiving.receive(None, send, cancelled).await });
    let mut messages = Vec::new();
    while messages.len() < 2 {
        let reading = tokio::time::timeout(Duration::from_secs(10), readings.recv())
            .await
            .unwrap()
            .unwrap();
        let ChannelReading::Batch {
            messages: next,
            protocol,
            accepted,
            ..
        } = reading
        else {
            panic!("unexpected reset")
        };
        assert!(protocol.is_none() && accepted.is_none());
        messages.extend(next);
    }
    assert_eq!(messages[0].input.conversation, "private:9");
    assert_eq!(messages[0].input.message, "-7");
    let ChannelAction::Message(content) = &messages[0].input.action else {
        panic!("expected message")
    };
    assert_eq!(content.text, "hello");
    assert_eq!(messages[1].input.conversation, "group:8");
    assert_eq!(messages[1].input.reply_to.as_deref(), Some("-501"));
    assert!(messages[1].input.addressed);
    assert!(matches!(messages[1].input.action, ChannelAction::Backtrack));
    stop.cancel();
    receiver.await.unwrap().unwrap();
}

async fn send_outputs(
    adapter: &dyn ChannelAdapter,
    requests: &mut mpsc::UnboundedReceiver<String>,
) {
    let file = FileData {
        name: "notes.txt".into(),
        data: b"saved".to_vec(),
    };
    let parts = adapter
        .split(Outgoing {
            text: "**hello**".into(),
            file: Some(file),
        })
        .await
        .unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(
        adapter
            .send("private:9", &parts[0])
            .await
            .unwrap()
            .message
            .as_deref(),
        Some("-501")
    );
    assert!(
        adapter
            .send("private:9", &parts[1])
            .await
            .unwrap()
            .message
            .is_none()
    );
    assert!(
        adapter
            .activity(
                "private:9",
                None,
                Some(&enco_kernel::ChannelActivity {
                    elapsed_seconds: 0,
                    tools: vec![]
                })
            )
            .await
            .unwrap()
            .unwrap()
            .message
            .is_none()
    );
    for route in [
        "/send_private_msg",
        "/upload_private_file",
        "/set_input_status",
    ] {
        assert_eq!(requests.recv().await.unwrap(), route);
    }
}

struct Server {
    url: String,
    requests: mpsc::UnboundedReceiver<String>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (send, requests) = mpsc::unbounded_channel();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    () = cancelled.cancelled() => break,
                    connection = listener.accept() => {
                        let (stream, _) = connection.unwrap();
                        connections.spawn(serve(stream, send.clone()));
                    }
                }
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            task,
        }
    }
}

async fn serve(stream: TcpStream, requests: mpsc::UnboundedSender<String>) {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    let route = line.split_whitespace().nth(1).unwrap().to_owned();
    let mut headers = BTreeMap::new();
    loop {
        line.clear();
        assert_ne!(stream.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.trim().split_once(':').unwrap();
        headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
    }
    assert_eq!(headers["authorization"], "Bearer 42:test-secret");
    if route == "/event" {
        let key = derive_accept_key(headers["sec-websocket-key"].as_bytes());
        stream.get_mut().write_all(format!("HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {key}\r\n\r\n").as_bytes()).await.unwrap();
        events(stream.into_inner()).await;
        return;
    }
    let length: usize = headers
        .get("content-length")
        .map_or(0, |length| length.parse().unwrap());
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await.unwrap();
    let data = match route.as_str() {
        "/get_login_info" => json!({"user_id":42,"nickname":"Fixture"}),
        "/send_private_msg" => json!({"message_id":-501}),
        "/upload_private_file" => json!({"file_id":"file"}),
        "/set_input_status" => Value::Null,
        _ => panic!("unexpected request: {route}"),
    };
    if route != "/get_login_info" {
        requests.send(route).unwrap();
    }
    let body = json!({"status":"ok","retcode":0,"data":data}).to_string();
    stream.get_mut().write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
}

async fn events(stream: TcpStream) {
    let mut socket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
    for frame in [
        json!({"post_type":"meta_event","meta_event_type":"lifecycle","sub_type":"connect","self_id":42}),
        json!({"post_type":"message_sent","message_type":"private","self_id":42}),
        json!({"post_type":"message","self_id":42,"message_type":"private","message_id":-7,"user_id":9,"sender":{"user_id":9,"nickname":"Owner"},"message":[{"type":"text","data":{"text":"hello"}}]}),
        json!({"post_type":"message","self_id":42,"message_type":"group","group_id":8,"message_id":-8,"user_id":9,"sender":{"user_id":9,"nickname":"Owner"},"message":[{"type":"reply","data":{"id":"-501"}},{"type":"at","data":{"qq":"42"}},{"type":"text","data":{"text":" /backtrack"}}]}),
    ] {
        socket.send(Message::text(frame.to_string())).await.unwrap();
    }
    let closed = socket.next().await.unwrap().unwrap();
    assert!(matches!(closed, Message::Close(_)));
    socket.flush().await.unwrap();
}
