//! Runs one protocol state machine and hands readings to the connection without waiting for preparation.
use super::{Connection, Transport, convert, wire};
use enco_core::{Failure, MappingOrigin, code};
use enco_kernel::{
    CHANNEL_MAX_BACKOFF, ChannelBackoff, ChannelCallError, ChannelFault, ChannelReading,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub(super) struct Receiver {
    pub transport: Arc<Transport>,
    identity: wire::Identity,
    state: Option<String>,
    readings: mpsc::UnboundedSender<ChannelReading>,
    acknowledgments: JoinSet<()>,
    pub stop: CancellationToken,
}

pub(super) struct Actions {
    pub frames: Vec<String>,
    pub timer: Option<u32>,
    pub reconnect: bool,
}

pub(super) async fn receive(
    connection: &Connection,
    protocol: Option<Value>,
    readings: mpsc::UnboundedSender<ChannelReading>,
    stop: CancellationToken,
) -> Result<(), ChannelCallError> {
    let mut receiver = Receiver {
        transport: Arc::clone(&connection.transport),
        identity: convert::identity(&connection.identity),
        state: protocol.map(|value| value.to_string()),
        readings,
        acknowledgments: JoinSet::new(),
        stop,
    };
    let result = receiver.run().await;
    // Confirmation has already reached the service; stopping waits for its result.
    while let Some(result) = receiver.acknowledgments.join_next().await {
        result.map_err(|error| task_error(format!("channel acknowledgment task: {error}")))?;
    }
    result
}

impl Receiver {
    /// Read until stopped.
    ///
    /// Retryable failures, and sockets that end before living as long as the longest backoff, wait
    /// a doubling backoff first, so a peer that keeps closing at once is not reconnected in a tight
    /// loop.
    async fn run(&mut self) -> Result<(), ChannelCallError> {
        let mut backoff = ChannelBackoff::default();
        while !self.stop.is_cancelled() {
            let delay = match self.read().await {
                Ok(Some(lived)) if lived < CHANNEL_MAX_BACKOFF => backoff.next(None),
                Ok(_) => {
                    backoff = ChannelBackoff::default();
                    continue;
                }
                Err(ChannelCallError::Protocol(error))
                    if !error.fatal && error.failure.retryable =>
                {
                    tracing::warn!(%error, "channel receive will retry");
                    backoff.next(error.retry_after)
                }
                Err(error) => return Err(error),
            };
            tokio::select! {
                () = self.stop.cancelled() => break,
                () = tokio::time::sleep(delay) => {}
            }
        }
        Ok(())
    }

    /// One read; for a socket, how long it stayed open.
    async fn read(&mut self) -> Result<Option<Duration>, ChannelCallError> {
        let next = self
            .transport
            .mapper
            .call(async |guest, store| {
                guest
                    .call_read(store, &self.identity, self.state.as_ref())
                    .await
            })
            .await
            .map_err(|failure| convert::failure(failure, false, None))?
            .map_err(|fault| convert::fault(fault, None));
        let next = match next {
            Err(fault)
                if fault.failure.code == code::CHANNEL_INVALID_STATE && self.state.is_some() =>
            {
                let (accepted, committed) = oneshot::channel();
                self.readings
                    .send(ChannelReading::Reset {
                        failure: fault.failure,
                        accepted,
                    })
                    .map_err(|_| task_error("channel preparation stopped"))?;
                self.accepted(committed).await?;
                self.state = None;
                return Ok(None);
            }
            other => other?,
        };
        match next {
            wire::ReadStep::Request(request) => self.request(request).await.map(|()| None),
            wire::ReadStep::Socket(socket) => {
                let opened = tokio::time::Instant::now();
                super::socket::receive(self, socket).await?;
                Ok(Some(opened.elapsed()))
            }
        }
    }

    async fn request(&mut self, request: wire::Request) -> Result<(), ChannelCallError> {
        let response = tokio::select! {
            () = self.stop.cancelled() => return Ok(()),
            result = self.transport.http(request) => result?,
        };
        let (accepted, committed) = oneshot::channel();
        self.arrived(wire::Arrival::Response(response), Some(accepted))
            .await?;
        // A request protocol may acknowledge its previous batch by making the next request.
        self.accepted(committed).await
    }

    async fn accepted(&self, committed: oneshot::Receiver<()>) -> Result<(), ChannelCallError> {
        tokio::select! {
            () = self.stop.cancelled() => Ok(()),
            result = committed => result.map_err(|_| task_error("channel reading was not accepted")),
        }
    }

    pub async fn arrived(
        &mut self,
        arrival: wire::Arrival,
        accepted: Option<oneshot::Sender<()>>,
    ) -> Result<Actions, ChannelCallError> {
        while let Some(result) = self.acknowledgments.try_join_next() {
            result.map_err(|error| task_error(format!("channel acknowledgment task: {error}")))?;
        }
        let origin = self.record(&arrival).await?;
        let reading = self
            .transport
            .mapper
            .call(async |guest, store| {
                guest
                    .call_received(store, &self.identity, self.state.as_ref(), &arrival)
                    .await
            })
            .await
            .map_err(|failure| convert::failure(failure, false, origin))?
            .map_err(|fault| convert::fault(fault, origin))?;
        let wire::Reading {
            messages,
            state,
            frames,
            acknowledgments,
            timer,
            reconnect,
        } = reading;
        if matches!(arrival, wire::Arrival::Response(_))
            && (!frames.is_empty() || timer.is_some() || reconnect)
        {
            return Err(task_error(
                "channel request reading returned socket actions",
            ));
        }
        let protocol = state
            .as_deref()
            .map(|state| convert::json(state, "channel protocol state"))
            .transpose()
            .map_err(|failure| convert::failure(failure, false, origin))?;
        let messages = messages
            .into_iter()
            .enumerate()
            .map(|(index, message)| {
                let origin = origin.ok_or_else(|| {
                    task_error("channel timer returned a message without external input")
                })?;
                let index = u32::try_from(index).map_err(|error| task_error(error.to_string()))?;
                convert::message(message, MappingOrigin { index, ..origin })
                    .map_err(|failure| convert::failure(failure, false, Some(origin)).into())
            })
            .collect::<Result<Vec<_>, ChannelCallError>>()?;
        if let Some(state) = state {
            self.state = Some(state);
        }
        let (confirmed, acknowledged) = oneshot::channel();
        if acknowledgments.is_empty() {
            let _ = confirmed.send(());
        } else {
            let transport = Arc::clone(&self.transport);
            self.acknowledgments.spawn(async move {
                futures_util::future::join_all(acknowledgments.into_iter().map(|request| {
                    let transport = &transport;
                    async move {
                        if let Err(error) = transport.notice(request).await {
                            tracing::warn!(%error, "channel acknowledgment failed");
                        }
                    }
                }))
                .await;
                let _ = confirmed.send(());
            });
        }
        self.readings
            .send(ChannelReading::Batch {
                messages,
                protocol,
                accepted,
                acknowledged,
            })
            .map_err(|_| task_error("channel preparation stopped"))?;
        Ok(Actions {
            frames,
            timer,
            reconnect,
        })
    }

    async fn record(
        &self,
        arrival: &wire::Arrival,
    ) -> Result<Option<MappingOrigin>, ChannelCallError> {
        let arrival = match arrival {
            wire::Arrival::Response(response) => {
                json!({"response": self.transport.response_record(response).await?})
            }
            wire::Arrival::Frame(frame) => json!({"frame": frame}),
            wire::Arrival::Closed(wire::Closure { code, reason }) => {
                json!({"closed": {"code": code, "reason": reason}})
            }
            wire::Arrival::Timer => return Ok(None),
        };
        self.transport.record("received", json!({
            "identity": {"protocol": self.identity.protocol, "account": self.identity.account, "facts": self.identity.facts},
            "state": self.state, "arrival": arrival,
        })).await.map(Some).map_err(Into::into)
    }
}

fn task_error(message: impl Into<String>) -> ChannelCallError {
    convert::failure(
        Failure {
            code: code::PLUGIN_CONTRACT.into(),
            message: message.into(),
            retryable: false,
        },
        false,
        None,
    )
    .into()
}

pub(super) fn network_error(message: String) -> ChannelFault {
    ChannelFault {
        failure: Failure {
            code: code::CHANNEL_NETWORK.into(),
            message,
            retryable: true,
        },
        unknown: false,
        fatal: false,
        retry_after: None,
        origin: None,
    }
}
