//! The silence limit, against a local server that paces its chunked body; these waits are real.
use super::*;
use std::io::{Read, Write};

const IDLE_MS: u32 = 300;

/// A server for one request that sends the head, then each chunk after `gap`, then ends the body.
fn server(chunks: &'static [&'static str], gap: Duration) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut byte = [0; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n")
            .unwrap();
        for chunk in chunks {
            std::thread::sleep(gap);
            write!(stream, "{:x}\r\n{chunk}\r\n", chunk.len()).unwrap();
        }
        std::thread::sleep(gap);
        // The peer may already have given up on a silent body.
        let _ = stream.write_all(b"0\r\n\r\n");
    });
    format!("http://{address}/")
}

async fn get(url: String) -> Result<host::HttpResponse, host::HttpFailure> {
    let request = host::HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![],
        body: None,
        timeout_ms: Some(60_000),
        idle_timeout_ms: Some(IDLE_MS),
    };
    http(&reqwest::Client::new(), &[], request).await
}

#[tokio::test]
async fn a_body_that_keeps_coming_outlasts_the_silence_limit() {
    let gap = Duration::from_millis(u64::from(IDLE_MS) / 4);
    let response = get(server(&["a", "b", "c", "d", "e", "f"], gap))
        .await
        .unwrap();
    assert_eq!(response.body, b"abcdef");
}

#[tokio::test]
async fn a_service_that_goes_quiet_is_a_timeout_that_may_be_retried() {
    let gap = Duration::from_millis(u64::from(IDLE_MS) * 3);
    let Err(failure) = get(server(&["a"], gap)).await else {
        panic!("a silent service answered");
    };
    assert!(matches!(failure.kind, host::HttpErrorKind::Timeout));
    assert!(failure.request_sent);
}
