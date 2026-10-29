//! Runs a mapper's `fetch` steps within the request limit and checks it ends in the kind asked for.
use super::{Connection, convert, host, wire};
use crate::limits::CHANNEL_FETCH_REQUESTS;
use enco_core::MappingOrigin;
use enco_kernel::{ChannelCallError, ChannelFault, ChannelQuotedMessage, Fetched};
use serde_json::json;

/// How a fetch ended, before checking that it is the kind the host asked for.
enum End {
    Image(Vec<u8>),
    Messages(Vec<wire::QuotedMessage>),
    Unavailable(String),
}

impl Connection {
    pub(super) async fn image(&self, locator: &str) -> Result<Fetched<Vec<u8>>, ChannelCallError> {
        let (end, origin) = self.fetch(locator).await?;
        let content = match end {
            End::Image(bytes) => Ok(bytes),
            End::Unavailable(reason) => Err(reason),
            End::Messages(_) => return Err(wrong_kind("an image", origin)),
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
            End::Image(_) => return Err(wrong_kind("messages", origin)),
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
            let response = match step {
                wire::FetchStep::Request(_) | wire::FetchStep::Download(_)
                    if responses.len() == limit as usize =>
                {
                    let failure = crate::convert::bad(format!(
                        "channel fetch asked for more than {limit} requests"
                    ));
                    return Err(convert::failure(failure, false, Some(origin)).into());
                }
                wire::FetchStep::Request(request) => {
                    transport.http(request).await.map_err(located)?
                }
                wire::FetchStep::Download(url) => {
                    // Content chose this address: outside the mapper's origins it only leaves
                    // the content unavailable, and it never carries the service's credentials.
                    let url = match transport.destinations.download(&url) {
                        Ok(url) => url,
                        Err(refused) => return Ok((End::Unavailable(refused.message), origin)),
                    };
                    let request = host::HttpRequest {
                        method: "GET".into(),
                        url: url.into(),
                        headers: Vec::new(),
                        body: None,
                        timeout_ms: None,
                    };
                    transport.exchange(request, &[]).await.map_err(located)?
                }
                wire::FetchStep::Image(bytes) => return Ok((End::Image(bytes), origin)),
                wire::FetchStep::Messages(messages) => {
                    return Ok((End::Messages(messages), origin));
                }
                wire::FetchStep::Unavailable(reason) => {
                    return Ok((End::Unavailable(reason), origin));
                }
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
