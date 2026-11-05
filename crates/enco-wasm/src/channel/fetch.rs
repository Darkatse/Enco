//! Runs a mapper's `fetch` steps within the request limit and checks it ends in the kind asked for.
use super::{Connection, convert, host, wire};
use crate::limits::CHANNEL_FETCH_REQUESTS;
use enco_core::MappingOrigin;
use enco_kernel::{ChannelCallError, ChannelFault, ChannelQuotedMessage, Fetched};
use serde_json::json;

/// How a fetch ended, before checking that it is the kind the host asked for.
enum End {
    File(Vec<u8>),
    Messages(Vec<wire::QuotedMessage>),
    Name(String),
    Unavailable(String),
}

impl Connection {
    pub(super) async fn file(&self, locator: &str) -> Result<Fetched<Vec<u8>>, ChannelCallError> {
        let (end, origin) = self.fetch(locator).await?;
        let content = match end {
            End::File(bytes) => Ok(bytes),
            End::Unavailable(reason) => Err(reason),
            End::Messages(_) | End::Name(_) => return Err(wrong_kind("a file", origin)),
        };
        Ok(Fetched {
            content,
            origin: Some(origin),
        })
    }

    pub(super) async fn quote(
        &self,
        locator: &str,
    ) -> Result<Fetched<Vec<ChannelQuotedMessage>>, ChannelCallError> {
        let (end, origin) = self.fetch(locator).await?;
        let content = match end {
            End::Messages(messages) => Ok(convert::quoted_messages(messages)
                .map_err(|failure| convert::failure(failure, false, Some(origin)))?),
            End::Unavailable(reason) => Err(reason),
            End::File(_) | End::Name(_) => return Err(wrong_kind("messages", origin)),
        };
        Ok(Fetched {
            content,
            origin: Some(origin),
        })
    }

    pub(super) async fn conversation_name(
        &self,
        locator: &str,
    ) -> Result<Fetched<String>, ChannelCallError> {
        let (end, origin) = self.fetch(locator).await?;
        let content = match end {
            End::Name(name) => {
                if name.is_empty() {
                    let failure = crate::convert::bad("fetched conversation name is empty");
                    return Err(convert::failure(failure, false, Some(origin)).into());
                }
                Ok(name)
            }
            End::Unavailable(reason) => Err(reason),
            End::File(_) | End::Messages(_) => {
                return Err(wrong_kind("a conversation name", origin));
            }
        };
        Ok(Fetched {
            content,
            origin: Some(origin),
        })
    }

    /// Record each step's arguments before calling the mapper, so the last record holds every
    /// response it read.
    async fn fetch(&self, locator: &str) -> Result<(End, MappingOrigin), ChannelCallError> {
        let transport = &self.transport;
        let limit = CHANNEL_FETCH_REQUESTS;
        let mut responses = Vec::new();
        let mut recorded = Vec::new();
        loop {
            let arguments = json!({"locator": locator, "responses": recorded, "limit": limit});
            let origin = transport.record("fetch", arguments).await?;
            let step = transport
                .mapper
                .call(async |guest, store| {
                    guest.call_fetch(store, locator, &responses, limit).await
                })
                .await
                .map_err(|failure| convert::failure(failure, false, Some(origin)))?
                .map_err(|fault| convert::fault(fault, Some(origin)))?;
            let located = |mut fault: ChannelFault| {
                fault.origin = Some(origin);
                fault
            };
            let exchanged = match step {
                wire::FetchStep::Request(_) | wire::FetchStep::Download(_)
                    if responses.len() == limit as usize =>
                {
                    let failure = crate::convert::bad(format!(
                        "channel fetch asked for more than {limit} requests"
                    ));
                    return Err(convert::failure(failure, false, Some(origin)).into());
                }
                wire::FetchStep::Request(request) => {
                    let request = super::request_for(&transport.destinations, request)
                        .map_err(|failure| convert::failure(failure, false, Some(origin)))?;
                    transport.exchange(request, &transport.credentials).await
                }
                wire::FetchStep::Download(url) => {
                    // Content chose this address: outside the mapper's origins it only leaves
                    // the content unavailable, and it never carries the service's credentials.
                    let Some(url) = transport.destinations.download(&url) else {
                        let reason = "Enco does not download files from this address";
                        return Ok((End::Unavailable(reason.into()), origin));
                    };
                    let request = host::HttpRequest {
                        method: "GET".into(),
                        url: url.into(),
                        headers: Vec::new(),
                        body: None,
                        timeout_ms: None,
                        idle_timeout_ms: None,
                    };
                    transport.exchange(request, &[]).await
                }
                wire::FetchStep::File(bytes) => return Ok((End::File(bytes), origin)),
                wire::FetchStep::Messages(messages) => {
                    return Ok((End::Messages(messages), origin));
                }
                wire::FetchStep::Name(name) => return Ok((End::Name(name), origin)),
                wire::FetchStep::Unavailable(reason) => {
                    return Ok((End::Unavailable(reason), origin));
                }
            };
            let response = match exchanged {
                Ok(response) => response,
                // The same request meets the same limit again, so the content stays unavailable
                // without a retry.
                Err(failure) if matches!(failure.kind, host::HttpErrorKind::TooLarge) => {
                    let reason = "too large for Enco to download";
                    return Ok((End::Unavailable(reason.into()), origin));
                }
                Err(failure) => return Err(located(super::fault(failure)).into()),
            };
            recorded.push(transport.response_record(&response).await?);
            responses.push(response);
        }
    }
}

fn wrong_kind(expected: &str, origin: MappingOrigin) -> ChannelCallError {
    let failure = crate::convert::bad(format!("channel fetch did not end in {expected}"));
    convert::failure(failure, false, Some(origin)).into()
}
