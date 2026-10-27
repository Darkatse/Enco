//! Executes pure channel mappers, recording response calls before invocation and owning HTTP secrets.
use crate::{
    bindings::{ChannelPluginPre, channels as wire, enco::plugin::host},
    call_world::CallWorld,
    channel_convert as convert,
    credentials::{self, CallCredential},
    host_imports::{self, HostState},
    origins::Destinations,
    plugin::WasmPlugin,
};
use async_trait::async_trait;
use enco_core::{
    ContentHash, Failure, FileData, GenerationId, MappingOrigin, MappingRecord, Outgoing, code,
};
use enco_kernel::{
    AttachmentContent, Channel, ChannelAdapter, ChannelCallError, ChannelCommand, ChannelFault,
    ChannelIdentity, ChannelReading, ChannelSent, ChannelSettings, Store, StoreError,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wasmtime::Store as WasmStore;

mod receiver;
mod socket;

#[derive(Clone)]
pub(super) struct WasmChannel {
    pre: ChannelPluginPre<HostState>,
    plugin: Arc<WasmPlugin>,
}

impl WasmChannel {
    pub(super) fn new(plugin: Arc<WasmPlugin>, pre: ChannelPluginPre<HostState>) -> Self {
        Self { pre, plugin }
    }

    /// Call one export on a fresh instance, without network, clocks or randomness that vary.
    async fn call<T>(
        &self,
        call: impl AsyncFnOnce(&wire::Guest, &mut WasmStore<HostState>) -> wasmtime::Result<T>,
    ) -> Result<T, Failure> {
        self.plugin
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let instance = self.pre.instantiate_async(&mut store).await?;
                Ok(Ok(call(instance.enco_plugin_channel(), &mut store).await?))
            })
            .await
    }
}

struct Transport {
    mapper: WasmChannel,
    destinations: Destinations,
    credentials: Vec<CallCredential>,
    generation: GenerationId,
    store: Arc<dyn Store>,
}

struct Connection {
    transport: Arc<Transport>,
    identity: ChannelIdentity,
}

#[async_trait]
impl Channel for WasmChannel {
    async fn open(
        &self,
        settings: &ChannelSettings,
        generation: GenerationId,
        store: Arc<dyn Store>,
    ) -> Result<Arc<dyn ChannelAdapter>, ChannelCallError> {
        // Registration checked the URL's form; opening only reads it.
        let base_url = reqwest::Url::parse(&settings.base_url).map_err(|error| {
            convert::failure(
                crate::convert::bad(format!("channel base URL: {error}")),
                false,
                None,
            )
        })?;
        let credentials =
            credentials::resolve_channel(&self.plugin.credentials, &base_url, &settings.secrets)
                .map_err(|failure| convert::failure(failure, false, None))?;
        let request = self
            .call(async |guest, store| guest.call_identify(store).await)
            .await
            .map_err(|failure| convert::failure(failure, false, None))?;
        let transport = Transport {
            mapper: self.clone(),
            destinations: Destinations::new(base_url, &self.plugin.channel_origins),
            credentials,
            generation,
            store,
        };
        let response = transport.http(request).await?;
        let origin = transport
            .record(
                "identified",
                json!({"response": transport.response_record(&response).await?}),
            )
            .await?;
        let identity = self
            .call(async |guest, store| guest.call_identified(store, &response).await)
            .await
            .map_err(|failure| convert::failure(failure, false, Some(origin)))?
            .map_err(|fault| convert::fault(fault, Some(origin)))?;
        let identity = convert::identified(identity)
            .map_err(|failure| convert::failure(failure, false, Some(origin)))?;
        Ok(Arc::new(Connection {
            transport: Arc::new(transport),
            identity,
        }))
    }
}

impl Transport {
    async fn record(&self, export: &str, arguments: Value) -> Result<MappingOrigin, StoreError> {
        let record = MappingRecord {
            generation: self.generation,
            export: format!("channel.{export}"),
            arguments,
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|error| StoreError::Backend(format!("mapping record: {error}")))?;
        Ok(MappingOrigin {
            call: self.store.put_blob(&bytes).await?,
            index: 0,
        })
    }

    async fn response_record(&self, response: &wire::Response) -> Result<Value, StoreError> {
        let wire::Response {
            status,
            headers,
            body,
        } = response;
        let body: ContentHash = self.store.put_blob(body).await?;
        Ok(json!({"status": status, "headers": headers, "body": body}))
    }

    async fn http(&self, request: wire::Request) -> Result<wire::Response, ChannelFault> {
        let request = request_for(&self.destinations, request)
            .map_err(|failure| convert::failure(failure, false, None))?;
        let response = host_imports::http(&self.mapper.plugin.http, &self.credentials, request)
            .await
            .map_err(|failure| {
                let invalid = matches!(failure.kind, host::HttpErrorKind::InvalidRequest);
                ChannelFault {
                    failure: Failure {
                        code: if invalid {
                            code::PLUGIN_CONTRACT
                        } else {
                            code::CHANNEL_NETWORK
                        }
                        .into(),
                        message: failure.message,
                        retryable: !invalid,
                    },
                    unknown: failure.request_sent,
                    fatal: invalid,
                    retry_after: None,
                    origin: None,
                }
            })?;
        let host::HttpResponse {
            status,
            headers,
            body,
        } = response;
        Ok(wire::Response {
            status,
            headers,
            body,
        })
    }

    async fn answered(&self, response: wire::Response) -> Result<ChannelSent, ChannelCallError> {
        let origin = self
            .record(
                "answered",
                json!({"response": self.response_record(&response).await?}),
            )
            .await?;
        let message = self
            .mapper
            .call(async |guest, store| guest.call_answered(store, &response).await)
            .await
            .map_err(|failure| convert::failure(failure, true, Some(origin)))?
            .map_err(|fault| convert::fault(fault, Some(origin)))?;
        Ok(ChannelSent { message, origin })
    }
}

#[async_trait]
impl ChannelAdapter for Connection {
    fn identity(&self) -> ChannelIdentity {
        self.identity.clone()
    }

    async fn receive(
        &self,
        protocol: Option<Value>,
        readings: mpsc::UnboundedSender<ChannelReading>,
        stop: CancellationToken,
    ) -> Result<(), ChannelCallError> {
        receiver::receive(self, protocol, readings, stop).await
    }

    async fn fetch(&self, locator: &str) -> Result<AttachmentContent, ChannelCallError> {
        let mut responses = Vec::new();
        let mut recorded = Vec::new();
        loop {
            let origin = self
                .transport
                .record("fetch", json!({"locator": locator, "responses": recorded}))
                .await?;
            let step = self
                .transport
                .mapper
                .call(async |guest, store| guest.call_fetch(store, locator, &responses).await)
                .await
                .map_err(|failure| convert::failure(failure, false, Some(origin)))?
                .map_err(|fault| convert::fault(fault, Some(origin)))?;
            match step {
                wire::FetchStep::Request(request) => {
                    let response = self.transport.http(request).await?;
                    recorded.push(self.transport.response_record(&response).await?);
                    responses.push(response);
                }
                wire::FetchStep::Image(bytes) => return Ok(AttachmentContent::Image(bytes)),
                wire::FetchStep::Unavailable(reason) => {
                    return Ok(AttachmentContent::Unavailable(reason));
                }
            }
        }
    }

    async fn split(
        &self,
        message: Outgoing<FileData>,
    ) -> Result<Vec<Outgoing<FileData>>, ChannelCallError> {
        let input = convert::outgoing(&message);
        let parts = self
            .transport
            .mapper
            .call(async |guest, store| guest.call_split(store, &input).await)
            .await
            .map_err(|failure| convert::failure(failure, false, None))?;
        convert::parts(parts, &message)
            .map_err(|failure| convert::failure(failure, false, None).into())
    }

    async fn send(
        &self,
        target: &str,
        message: &Outgoing<FileData>,
    ) -> Result<ChannelSent, ChannelCallError> {
        let message = convert::outgoing(message);
        let request = self
            .transport
            .mapper
            .call(async |guest, store| guest.call_send(store, target, &message).await)
            .await
            .map_err(|failure| convert::failure(failure, false, None))?
            .map_err(|fault| convert::fault(fault, None))?;
        self.transport
            .answered(self.transport.http(request).await?)
            .await
    }

    async fn commands(&self, commands: &[ChannelCommand]) -> Result<(), ChannelCallError> {
        let commands = commands.iter().map(convert::command).collect::<Vec<_>>();
        let identity = convert::identity(&self.identity);
        let requests = self
            .transport
            .mapper
            .call(async |guest, store| guest.call_commands(store, &identity, &commands).await)
            .await
            .map_err(|failure| convert::failure(failure, false, None))?;
        for request in requests {
            self.transport
                .answered(self.transport.http(request).await?)
                .await?;
        }
        Ok(())
    }

    async fn respond(&self, handle: &str, text: Option<&str>) -> Result<(), ChannelCallError> {
        let request = self
            .transport
            .mapper
            .call(async |guest, store| guest.call_respond(store, handle, text).await)
            .await
            .map_err(|failure| convert::failure(failure, false, None))?
            .map_err(|fault| convert::fault(fault, None))?;
        if let Some(request) = request {
            self.transport.notice(request).await?;
        }
        Ok(())
    }

    async fn activity(
        &self,
        target: &str,
        shown: Option<&str>,
        activity: Option<&enco_kernel::ChannelActivity>,
    ) -> Result<Option<ChannelSent>, ChannelCallError> {
        let activity = activity.map(convert::activity);
        let request = self
            .transport
            .mapper
            .call(async |guest, store| {
                guest
                    .call_activity(store, target, shown, activity.as_ref())
                    .await
            })
            .await
            .map_err(|failure| convert::failure(failure, false, None))?;
        let Some(request) = request else {
            return Ok(None);
        };
        if shown.is_none() && activity.is_some() {
            return self
                .transport
                .answered(self.transport.http(request).await?)
                .await
                .map(Some);
        }
        self.transport.notice(request).await?;
        Ok(None)
    }
}

impl Transport {
    async fn notice(&self, request: wire::Request) -> Result<(), ChannelFault> {
        let wire::Response { status, .. } = self.http(request).await?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(ChannelFault {
            failure: Failure {
                code: format!("channel.http.{status}"),
                message: format!("channel notice returned HTTP {status}"),
                retryable: false,
            },
            unknown: false,
            fatal: false,
            retry_after: None,
            origin: None,
        })
    }
}

fn request_for(
    destinations: &Destinations,
    request: wire::Request,
) -> Result<host::HttpRequest, Failure> {
    let wire::Request {
        method,
        url,
        headers,
        body,
        timeout_ms,
    } = request;
    Ok(host::HttpRequest {
        method,
        url: destinations.request(&url)?.to_string(),
        headers,
        body,
        timeout_ms,
    })
}
