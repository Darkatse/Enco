//! A bot request remains pending until the test replies or observes client cancellation.
use serde_json::Value;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct Request {
    pub path: String,
    pub body: Value,
    pub reply: oneshot::Sender<Value>,
    pub closed: oneshot::Receiver<()>,
}

pub struct Bot {
    pub url: String,
    requests: mpsc::UnboundedReceiver<Request>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Bot {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (send, requests) = mpsc::unbounded_channel();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancelled.cancelled() => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        connections.spawn(request(stream, send.clone()));
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

    pub async fn next(&mut self) -> Request {
        tokio::time::timeout(Duration::from_secs(15), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }

    pub async fn shutdown(self) {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(15), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

async fn request(stream: TcpStream, requests: mpsc::UnboundedSender<Request>) {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    let path = line.split_whitespace().nth(1).unwrap().to_owned();
    let mut length = None;
    loop {
        line.clear();
        assert!(stream.read_line(&mut line).await.unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut bytes = vec![0; length.unwrap()];
    stream.read_exact(&mut bytes).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap();
    let (reply, response) = oneshot::channel();
    let (closed, disconnected) = oneshot::channel();
    requests
        .send(Request {
            path,
            body,
            reply,
            closed: disconnected,
        })
        .unwrap();
    let mut byte = [0];
    tokio::select! {
        value = response => {
            let body = value.unwrap().to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.get_mut().write_all(response.as_bytes()).await.unwrap();
            stream.get_mut().shutdown().await.unwrap();
        }
        read = stream.read(&mut byte) => { assert_eq!(read.unwrap(), 0); }
    }
    let _ = closed.send(());
}
