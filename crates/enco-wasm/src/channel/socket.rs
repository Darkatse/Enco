//! Holds the socket and one timer; only the mapper interprets frames and decides to reconnect.
use super::{
    credentials,
    receiver::{Receiver, network_error},
    wire,
};
use enco_kernel::ChannelCallError;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{net::TcpStream, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Message, client::IntoClientRequest, handshake::client::Request, protocol::CloseFrame,
    },
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(super) async fn receive(
    receiver: &mut Receiver,
    settings: wire::Socket,
) -> Result<(), ChannelCallError> {
    let url = receiver
        .transport
        .destinations
        .socket(&settings.url)
        .map_err(|failure| super::convert::failure(failure, false, None))?;
    let request = opening_request(&receiver.transport.credentials, &url)
        .map_err(|failure| super::convert::failure(failure, false, None))?;
    let opened = tokio::select! {
        () = receiver.stop.cancelled() => return Ok(()),
        result = tokio::time::timeout(crate::limits::HTTP_DEFAULT_TIMEOUT, tokio_tungstenite::connect_async(request)) => result,
    };
    let (mut socket, _) = opened
        .map_err(|_| network_error("channel socket connection timed out".into()))?
        .map_err(|error| {
            network_error(credentials::redact_text(
                &receiver.transport.credentials,
                &error.to_string(),
            ))
        })?;
    let result = run(receiver, &mut socket, &url).await;
    let close = CloseFrame {
        code: settings.close_code.into(),
        reason: "".into(),
    };
    // The requested code lets resumable protocols distinguish this from ending the session.
    match tokio::time::timeout(
        crate::limits::HTTP_DEFAULT_TIMEOUT,
        socket.close(Some(close)),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::debug!(error = %credentials::redact_text(&receiver.transport.credentials, &error.to_string()), "channel socket closed before close frame")
        }
        Err(_) => tracing::warn!("channel socket close timed out"),
    }
    result
}

fn opening_request(
    credentials: &[credentials::CallCredential],
    url: &reqwest::Url,
) -> Result<Request, enco_core::Failure> {
    let mut request = reqwest::Request::new(reqwest::Method::GET, url.clone());
    for credential in credentials {
        credential.attach(&mut request);
    }
    let mut opening = request
        .url()
        .as_str()
        .into_client_request()
        .map_err(|error| {
            crate::convert::bad(credentials::redact_text(credentials, &error.to_string()))
        })?;
    opening
        .headers_mut()
        .extend(std::mem::take(request.headers_mut()));
    Ok(opening)
}

async fn run(
    receiver: &mut Receiver,
    socket: &mut Socket,
    url: &reqwest::Url,
) -> Result<(), ChannelCallError> {
    let mut deadline = None;
    loop {
        let arrival = tokio::select! {
            () = receiver.stop.cancelled() => return Ok(()),
            () = timer(deadline) => { deadline = None; wire::Arrival::Timer },
            frame = socket.next() => match frame {
                Some(Ok(Message::Text(text))) => wire::Arrival::Frame(credentials::redact_text(&receiver.transport.credentials, &text)),
                Some(Ok(Message::Close(close))) => wire::Arrival::Closed(wire::Closure {
                    code: close.as_ref().map(|close| close.code.into()),
                    reason: close.map_or_else(String::new, |close| credentials::redact_text(&receiver.transport.credentials, &close.reason)),
                }),
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
                Some(Ok(Message::Binary(_))) => return Err(super::convert::failure(
                    crate::convert::bad("channel socket returned a binary frame; the channel contract accepts text"), false, None).into()),
                Some(Err(error)) => wire::Arrival::Closed(wire::Closure {
                    code: None,
                    reason: credentials::redact_text(&receiver.transport.credentials, &error.to_string()),
                }),
                None => wire::Arrival::Closed(wire::Closure { code: None, reason: "socket ended".into() }),
            },
        };
        let closed = matches!(arrival, wire::Arrival::Closed(_));
        let actions = receiver.arrived(arrival, None).await?;
        for frame in actions.frames {
            let frame = credentials::attach_frame(&receiver.transport.credentials, url, &frame)
                .map_err(|failure| super::convert::failure(failure, false, None))?;
            let sent = tokio::select! {
                () = receiver.stop.cancelled() => return Ok(()),
                result = socket.send(Message::Text(frame.into())) => result,
            };
            if let Err(error) = sent {
                receiver
                    .arrived(
                        wire::Arrival::Closed(wire::Closure {
                            code: None,
                            reason: credentials::redact_text(
                                &receiver.transport.credentials,
                                &error.to_string(),
                            ),
                        }),
                        None,
                    )
                    .await?;
                return Ok(());
            }
        }
        if let Some(milliseconds) = actions.timer {
            deadline = Some(Instant::now() + Duration::from_millis(milliseconds.into()));
        }
        if actions.reconnect || closed {
            return Ok(());
        }
    }
}

async fn timer(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
