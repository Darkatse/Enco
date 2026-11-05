//! Records the external input a pure mapper is about to read, so every output keeps its source.
//!
//! A record holds the generation, the export and its complete arguments, with each response body
//! stored as evidence of its own; channel mappers and Providers share this one form.
use crate::bindings::enco::plugin::types::HttpResponse;
use enco_core::{ContentHash, GenerationId, MappingOrigin, MappingRecord};
use enco_kernel::{Store, StoreError};
use serde_json::{Value, json};

/// Store the record of one call to `export` before it runs, and return where its outputs come
/// from; a call with several outputs indexes them from this origin.
pub(crate) async fn record(
    store: &dyn Store,
    generation: GenerationId,
    export: String,
    arguments: Value,
) -> Result<MappingOrigin, StoreError> {
    let record = MappingRecord {
        generation,
        export,
        arguments,
    };
    let bytes = serde_json::to_vec(&record)
        .map_err(|error| StoreError::Backend(format!("mapping record: {error}")))?;
    Ok(MappingOrigin {
        call: store.put_evidence(&bytes).await?,
        index: 0,
    })
}

/// A response as a record's argument, with its body replaced by the address of the stored bytes.
pub(crate) async fn response(
    store: &dyn Store,
    response: &HttpResponse,
) -> Result<Value, StoreError> {
    let HttpResponse {
        status,
        headers,
        body,
    } = response;
    let body: ContentHash = store.put_evidence(body).await?;
    Ok(json!({"status": status, "headers": headers, "body": body}))
}
