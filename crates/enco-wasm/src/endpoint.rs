//! Runs the mappers an endpoint names, for completion, embedding and decision: encodes, sends to
//! the endpoint, then decodes.
//!
//! Encoding and decoding each run on a fresh instance; the request carries the endpoint's
//! credentials. These mappers import nothing, so every network effect of their calls happens here.
//! Only a completion's response is recorded before it is decoded, since only its settlement enters
//! the Log.
use crate::{
    bindings::enco::plugin::{host, types as wit},
    call_world::CallWorld,
    convert, credentials, host_imports, mapping,
    origins::Destinations,
    plugin::{WasmPlugin, trap},
};
use async_trait::async_trait;
use enco_core::*;
use enco_kernel::{
    Answer, Answered, CompleteError, Decision, Embedding, Provider, ProviderRequest, Question,
    Recording, StoreError,
};
use serde_json::json;

/// The export a completion's mapping record names.
const DECODE: &str = "completion.decode";

#[async_trait]
impl Provider for WasmPlugin {
    async fn complete(
        &self,
        params: &ProviderSettings,
        request: ProviderRequest,
        record: Option<Recording>,
    ) -> Result<Answered, StoreError> {
        let unanswered = |failure| {
            Ok(Answered {
                result: Err(CompleteError::Failed(failure)),
                origin: None,
            })
        };
        let Some(pre) = &self.completion else {
            return unanswered(trap("completion export is missing"));
        };
        let settings = convert::settings(params);
        let request = convert::request(request);
        let encoded = self
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_completion()
                    .call_encode(&mut store, &settings, &request)
                    .await
            })
            .await;
        let response = match encoded {
            Ok(encoded) => self.send(params, encoded).await,
            Err(failure) => Err(failure),
        };
        let response = match response {
            Ok(response) => response,
            Err(failure) => return unanswered(failure),
        };
        let origin = match record {
            Some(Recording { generation, store }) => {
                let arguments =
                    json!({"response": mapping::response(store.as_ref(), &response).await?});
                Some(mapping::record(store.as_ref(), generation, DECODE.into(), arguments).await?)
            }
            None => None,
        };
        // The service's refusal is an answer to convert, not a failure of the invocation.
        let decoded = self
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_completion()
                    .call_decode(&mut store, &response)
                    .await
                    .map(Ok)
            })
            .await;
        let result = match decoded {
            Ok(Ok(completion)) => convert::completion(completion).map_err(CompleteError::Failed),
            Ok(Err(error)) => {
                Err(convert::complete_error(error).unwrap_or_else(CompleteError::Failed))
            }
            Err(failure) => Err(CompleteError::Failed(failure)),
        };
        Ok(Answered { result, origin })
    }
}

#[async_trait]
impl Embedding for WasmPlugin {
    async fn embed(
        &self,
        params: &ProviderSettings,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, Failure> {
        let pre = self
            .embedding
            .as_ref()
            .ok_or_else(|| trap("embedding export is missing"))?;
        let count = u32::try_from(inputs.len()).map_err(|_| Failure {
            code: code::PROVIDER_BAD_REQUEST.into(),
            message: "too many inputs to embed in one request".into(),
            retryable: false,
        })?;
        let settings = convert::settings(params);
        let request = self
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_embedding()
                    .call_encode(&mut store, &settings, &inputs)
                    .await
            })
            .await?;
        let response = self.send(params, request).await?;
        let vectors = self
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_embedding()
                    .call_decode(&mut store, count, &response)
                    .await
            })
            .await?;
        convert::embeddings(vectors, count)
    }
}

#[async_trait]
impl Decision for WasmPlugin {
    async fn decide(
        &self,
        params: &ProviderSettings,
        state: String,
        questions: Vec<Question>,
    ) -> Result<Vec<Answer>, Failure> {
        let pre = self
            .decision
            .as_ref()
            .ok_or_else(|| trap("decision export is missing"))?;
        let settings = convert::settings(params);
        let asked: Vec<_> = questions.iter().map(convert::question).collect();
        let request = self
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_decision()
                    .call_encode(&mut store, &settings, &state, &asked)
                    .await
            })
            .await?;
        let response = self.send(params, request).await?;
        let answers = self
            .invoke(CallWorld::Mapper, Default::default(), async |mut store| {
                let plugin = pre.instantiate_async(&mut store).await?;
                plugin
                    .enco_plugin_decision()
                    .call_decode(&mut store, &asked, &response)
                    .await
            })
            .await?;
        convert::answers(answers, &questions)
    }
}

impl WasmPlugin {
    /// Send a mapper's request to the endpoint with the endpoint's credentials.
    ///
    /// A request the mapper built wrongly, such as one for another origin, is `plugin.contract`;
    /// one that gets no response is a retryable `provider.network` or `timeout`, and a response
    /// too large to read is `provider.bad_response`.
    async fn send(
        &self,
        params: &ProviderSettings,
        request: wit::HttpRequest,
    ) -> Result<wit::HttpResponse, Failure> {
        let credentials = credentials::resolve(&self.credentials, &self.settings, Some(params))?;
        let url = Destinations::new(&params.base_url, &[]).request(&request.url)?;
        let request = wit::HttpRequest {
            url: url.to_string(),
            ..request
        };
        host_imports::http(&self.http, &credentials, request)
            .await
            .map_err(|failure| {
                let (code, retryable) = match failure.kind {
                    host::HttpErrorKind::InvalidRequest => return convert::bad(failure.message),
                    host::HttpErrorKind::Timeout => (code::TIMEOUT, true),
                    // The response arrived and asking again would bring the same one back.
                    host::HttpErrorKind::TooLarge => (code::PROVIDER_BAD_RESPONSE, false),
                    _ => (code::PROVIDER_NETWORK, true),
                };
                Failure {
                    code: code.into(),
                    message: failure.message,
                    retryable,
                }
            })
    }
}
