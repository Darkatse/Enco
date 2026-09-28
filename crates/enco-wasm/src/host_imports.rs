use crate::{bindings::enco::plugin::host, limits};
use std::time::Duration;
use wasmtime::component::{Accessor, HasSelf, ResourceTable};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

pub(crate) struct HostState {
    pub wasi: WasiCtx,
    pub table: ResourceTable,
    pub http: reqwest::Client,
    pub name: String,
    pub limits: wasmtime::StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl host::Host for HostState {
    fn log(&mut self, level: host::LogLevel, message: String) {
        match level {
            host::LogLevel::Trace => {
                tracing::trace!(target: "plugin", plugin = %self.name, "{message}")
            }
            host::LogLevel::Debug => {
                tracing::debug!(target: "plugin", plugin = %self.name, "{message}")
            }
            host::LogLevel::Info => {
                tracing::info!(target: "plugin", plugin = %self.name, "{message}")
            }
            host::LogLevel::Warn => {
                tracing::warn!(target: "plugin", plugin = %self.name, "{message}")
            }
            host::LogLevel::Error => {
                tracing::error!(target: "plugin", plugin = %self.name, "{message}")
            }
        }
    }
}

impl<U> host::HostWithStore<U> for HasSelf<HostState> {
    async fn http(
        accessor: &Accessor<U, Self>,
        request: host::HttpRequest,
    ) -> Result<host::HttpResponse, host::HttpFailure> {
        let client = accessor.with(|mut access| access.get().http.clone());
        http(&client, request).await
    }
}

fn failure(
    kind: host::HttpErrorKind,
    message: impl Into<String>,
    request_sent: bool,
) -> host::HttpFailure {
    host::HttpFailure {
        kind,
        message: message.into(),
        request_sent,
    }
}

async fn http(
    client: &reqwest::Client,
    request: host::HttpRequest,
) -> Result<host::HttpResponse, host::HttpFailure> {
    let request = build_request(client, request)?;
    let mut response = client.execute(request).await.map_err(|e| {
        let (kind, request_sent) = if e.is_timeout() {
            (host::HttpErrorKind::Timeout, true)
        } else if e.is_connect() {
            (host::HttpErrorKind::Connect, false)
        } else {
            (host::HttpErrorKind::Other, true)
        };
        failure(kind, e.without_url().to_string(), request_sent)
    })?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let mut body = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        failure(
            if e.is_timeout() {
                host::HttpErrorKind::Timeout
            } else {
                host::HttpErrorKind::Body
            },
            e.without_url().to_string(),
            true,
        )
    })? {
        if body.len() + chunk.len() > limits::HTTP_MAX_BODY_BYTES {
            return Err(failure(
                host::HttpErrorKind::TooLarge,
                "HTTP response exceeds 32 MiB",
                true,
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(host::HttpResponse {
        status,
        headers,
        body,
    })
}

impl crate::bindings::enco::plugin::types::Host for HostState {}

fn build_request(
    client: &reqwest::Client,
    request: host::HttpRequest,
) -> Result<reqwest::Request, host::HttpFailure> {
    let invalid = |e: String| failure(host::HttpErrorKind::InvalidRequest, e, false);
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .map_err(|e| invalid(e.to_string()))?;
    let url = reqwest::Url::parse(&request.url).map_err(|e| invalid(e.to_string()))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(invalid("HTTP requires http or https".into()));
    }
    let mut builder = client.request(method, url).timeout(
        request
            .timeout_ms
            .map(|ms| Duration::from_millis(ms.into()))
            .unwrap_or(limits::HTTP_DEFAULT_TIMEOUT),
    );
    for (name, value) in request.headers {
        builder = builder.header(name, value);
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    builder.build().map_err(|error| invalid(error.to_string()))
}
