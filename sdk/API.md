<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_sdk`

`src/lib.rs`

Rust bindings for the plugin contract, the `export!` macro, and helpers for plugins that call
JSON services over HTTP.

The host writes this crate to `sdk/` in every home, replacing any edits there; plugins depend on
it by path.

- module `bindings` — Types and host functions generated from the WIT.
- module `host` — Host-owned logging and transport available to plugins.
- module `protocol` — Helpers for plugins that call model services through the host's `http` import, including OpenAI-compatible Chat Completions and embeddings.
- module `types` — Canonical data exchanged between the host and plugins.
- macro `export` — Export `$implementation` as the plugin for one world: `completion`, `provider` (completion and embedding), `decision` or `tools`.
- fn `input_schema` — The canonical argument schema of `T`; a `types::ToolSpec` takes it as JSON text.
- fn `parse_arguments` — Parse a tool call's `input` into `T`; a failure has code `tool.invalid_arguments` and is not retryable.

Re-exports `wit_bindgen`.

## macro `export`

`src/lib.rs`

```rust
macro_rules! export {
    (completion, $implementation:ident) => { ... };
    (provider, $implementation:ident) => { ... };
    (decision, $implementation:ident) => { ... };
    (tools, $implementation:ident) => { ... };
    (@provider $world:literal, $implementation:ident) => { ... };
    (@world $world:literal, $implementation:ident, $($types:tt)*) => { ... };
}
```

Export `$implementation` as the plugin for one world: `completion`, `provider` (completion and
embedding), `decision` or `tools`.

Call it in a crate in `plugins/<name>` that depends on this SDK as `enco_sdk`. It defines a
`bindings` module beside the call, whose `exports` hold the `Guest` traits to implement; the
WIT is read from `../../sdk/wit`.

## fn `input_schema`

`src/lib.rs`

```rust
pub fn input_schema<T: schemars::JsonSchema>() -> serde_json::Value
```

The canonical argument schema of `T`; a `types::ToolSpec` takes it as JSON text.

Field docs become the parameter descriptions the model reads. Every struct in `T` needs
`#[serde(deny_unknown_fields)]`, because the host accepts only closed objects. Maps, sets and
recursive types fall outside the subset the host accepts, so a plugin that uses them fails to
load.

## fn `parse_arguments`

`src/lib.rs`

```rust
pub fn parse_arguments<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, types::Failure>
```

Parse a tool call's `input` into `T`; a failure has code `tool.invalid_arguments` and is not
retryable.

The host has already checked `input` against the tool's declared schema, so this fails only on
rules the type enforces itself, such as a custom `Deserialize`.

# module `enco_sdk::bindings`

`src/lib.rs`

Types and host functions generated from the WIT.

Plugins built with `export!` use these same types, so a value from `protocol` can be
returned from a plugin as it is.

- module `bindings::enco` — (no summary)
- module `bindings::exports` — (no summary)

# module `enco_sdk::protocol`

`src/protocol.rs`

Helpers for plugins that call model services through the host's `http` import, including
OpenAI-compatible Chat Completions and embeddings.

- module `protocol::host` — same as `host`
- module `protocol::types` — same as `types`
- struct `protocol::Completion` — A completed model attempt. `message.role` must be assistant. Canonical parts are text, tool calls and extensions; generated images stay in extensions.
- struct `protocol::Request` — The frozen messages, disclosed tools and output budget for one completion. User messages may include images; tool messages contain one result followed by any images.
- struct `protocol::Settings` — same as `types::Settings`
- enum `protocol::StopReason` — The service-reported reason that generation stopped.
- struct `protocol::Usage` — Token counts reported by the service, including cache hits when available.
- fn `protocol::apply_options` — Merge `raw`, a JSON object of caller options, into `body`; options may only add fields, and never one named in `reserved`.
- fn `protocol::bad` — A non-retryable `provider.bad_response` failure: the reply is malformed or not what the protocol promises.
- fn `protocol::bad_request` — A non-retryable `provider.bad_request` failure: the request cannot succeed as written.
- fn `protocol::complete` — Send `request` as one non-streaming Chat Completions call and return the reply as a `Completion`.
- fn `protocol::embed` — Embed each input through the `embeddings` endpoint, returning one vector per input, in input order.
- fn `protocol::empty_config` — Accept only `{}` as the plugin's configuration; anything else fails with `config.invalid`.
- fn `protocol::failure` — A failure with the given code; `retryable` means the same request may succeed later.
- fn `protocol::post` — POST `body` as JSON to `path` under `settings.base_url` and return the JSON reply.

## struct `protocol::Completion`

`src/lib.rs`

```rust
pub struct Completion {
    pub message: Message,
    pub usage: Usage,
    pub stop_reason: StopReason,
}
```

A completed model attempt. `message.role` must be assistant.
Canonical parts are text, tool calls and extensions; generated images stay in extensions.

Implements `Clone`, `Debug`.

## struct `protocol::Request`

`src/lib.rs`

```rust
pub struct Request {
    pub messages: _rt::Vec<Message>,
    pub tools: _rt::Vec<ToolSpec>,
    pub max_output_tokens: ::core::option::Option<u32>,
}
```

The frozen messages, disclosed tools and output budget for one completion.
User messages may include images; tool messages contain one result followed by any images.

Implements `Clone`, `Debug`.

## enum `protocol::StopReason`

`src/lib.rs`

```rust
pub enum StopReason {
    EndTurn,
    ToolCalls,
    MaxTokens,
    Other,
}
```

The service-reported reason that generation stopped.

Implements `Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`.

## struct `protocol::Usage`

`src/lib.rs`

```rust
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: ::core::option::Option<u64>,
}
```

Token counts reported by the service, including cache hits when available.

Implements `Clone`, `Copy`, `Debug`.

## fn `protocol::apply_options`

`src/http.rs`

```rust
pub fn apply_options(body: &mut serde_json::Value, raw: &str, reserved: &[&str]) -> Result<(), crate::protocol::types::Failure>
```

Merge `raw`, a JSON object of caller options, into `body`; options may only add fields, and
never one named in `reserved`.

A conflict or invalid JSON fails as `provider.bad_request`.

## fn `protocol::bad`

`src/protocol.rs`

```rust
pub fn bad(message: impl Into<String>) -> types::Failure
```

A non-retryable `provider.bad_response` failure: the reply is malformed or not what the protocol
promises.

## fn `protocol::bad_request`

`src/protocol.rs`

```rust
pub fn bad_request(message: impl Into<String>) -> types::Failure
```

A non-retryable `provider.bad_request` failure: the request cannot succeed as written.

## fn `protocol::complete`

`src/protocol.rs`

```rust
pub async fn complete(settings: Settings, request: Request) -> Result<Completion, types::Failure>
```

Send `request` as one non-streaming Chat Completions call and return the reply as a
`Completion`.

`settings.options` may add fields to the request, but not replace one this function writes or
reserves, such as `model`, `messages` or `stream`. Reply fields the contract has no place for
are kept in an extension part and sent back with that message later.

## fn `protocol::embed`

`src/protocol.rs`

```rust
pub async fn embed(settings: Settings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, types::Failure>
```

Embed each input through the `embeddings` endpoint, returning one vector per input, in input
order.

The request times out after 30 seconds. The reply must hold exactly one non-empty vector of
finite numbers for each input, all of the same length; otherwise the call fails as
`provider.bad_response`.

## fn `protocol::empty_config`

`src/protocol.rs`

```rust
pub fn empty_config(config: &str) -> Result<(), types::Failure>
```

Accept only `{}` as the plugin's configuration; anything else fails with `config.invalid`.

Use it in plugins whose endpoint, model and options all arrive with each call's `Settings`.

## fn `protocol::failure`

`src/protocol.rs`

```rust
pub fn failure(code: &str, message: impl Into<String>, retryable: bool) -> types::Failure
```

A failure with the given code; `retryable` means the same request may succeed later.

## fn `protocol::post`

`src/http.rs`

```rust
pub async fn post(settings: &crate::protocol::Settings, path: &str, body: serde_json::Value, timeout_ms: Option<u32>) -> Result<serde_json::Value, crate::protocol::types::Failure>
```

POST `body` as JSON to `path` under `settings.base_url` and return the JSON reply.

`None` for `timeout_ms` means the host's default. Retryable: a failed `http` call, 408, 429 and
5xx. Not retryable: 401 and 403 (`provider.auth`), any other status outside 2xx
(`provider.bad_request`), and a reply that is not JSON (`provider.bad_response`).

# module `enco_sdk::host`

`src/lib.rs`

Host-owned logging and transport available to plugins.

- enum `host::HttpErrorKind` — Classifies failures while constructing, sending or reading an HTTP request.
- struct `host::HttpFailure` — Transport failure. `request-sent` means the request may have reached the peer.
- struct `host::HttpRequest` — An HTTP request. Omitting `timeout-ms` selects the host default timeout.
- struct `host::HttpResponse` — An HTTP status, headers and fully collected response body.
- enum `host::LogLevel` — Severity attached to a plugin diagnostic.
- fn `host::config` — The configuration of this invocation's generation, excluding credential mappings.
- fn `host::http` — Send a request and collect its response. Non-2xx statuses are returned as responses. The host attaches declared credentials only to the bound origin (scheme, host and port), overriding the matching request header. Redirects are followed only within that origin. Cross-origin redirects are returned unchanged. Credential values in response headers, bodies and failures are replaced with [redacted:<environment variable name>]. Encoded forms of a credential (such as base64) are not redacted.
- fn `host::log` — Write a diagnostic to the host log.
- type `host::Json` — (no summary)

## enum `host::HttpErrorKind`

`src/lib.rs`

```rust
pub enum HttpErrorKind {
    InvalidRequest,
    Connect,
    Timeout,
    Body,
    TooLarge,
    Other,
}
```

Classifies failures while constructing, sending or reading an HTTP request.

Implements `Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`.

## struct `host::HttpFailure`

`src/lib.rs`

```rust
pub struct HttpFailure {
    pub kind: HttpErrorKind,
    pub message: _rt::String,
    pub request_sent: bool,
}
```

Transport failure. `request-sent` means the request may have reached the peer.

Implements `Clone`, `Debug`, `Display`, `Error`.

## struct `host::HttpRequest`

`src/lib.rs`

```rust
pub struct HttpRequest {
    pub method: _rt::String,
    pub url: _rt::String,
    pub headers: _rt::Vec<(_rt::String, _rt::String)>,
    pub body: ::core::option::Option<_rt::Vec<u8>>,
    pub timeout_ms: ::core::option::Option<u32>,
}
```

An HTTP request. Omitting `timeout-ms` selects the host default timeout.

Implements `Clone`, `Debug`.

## struct `host::HttpResponse`

`src/lib.rs`

```rust
pub struct HttpResponse {
    pub status: u16,
    pub headers: _rt::Vec<(_rt::String, _rt::String)>,
    pub body: _rt::Vec<u8>,
}
```

An HTTP status, headers and fully collected response body.

Implements `Clone`, `Debug`.

## enum `host::LogLevel`

`src/lib.rs`

```rust
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}
```

Severity attached to a plugin diagnostic.

Implements `Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`.

## fn `host::config`

`src/lib.rs`

```rust
pub fn config() -> Json
```

The configuration of this invocation's generation, excluding credential mappings.

## fn `host::http`

`src/lib.rs`

```rust
pub async fn http(request: HttpRequest) -> ::core::result::Result<HttpResponse, HttpFailure>
```

Send a request and collect its response. Non-2xx statuses are returned as responses.
The host attaches declared credentials only to the bound origin (scheme, host and port),
overriding the matching request header. Redirects are followed only within that origin.
Cross-origin redirects are returned unchanged. Credential values in response headers,
bodies and failures are replaced with [redacted:<environment variable name>].
Encoded forms of a credential (such as base64) are not redacted.

## fn `host::log`

`src/lib.rs`

```rust
pub fn log(level: LogLevel, message: &str)
```

Write a diagnostic to the host log.

## type `host::Json`

`src/lib.rs`

```rust
pub type Json = super::super::super::enco::plugin::types::Json;
```

# module `enco_sdk::types`

`src/lib.rs`

Canonical data exchanged between the host and plugins.

- enum `types::Effect` — Mutation semantics used to order calls and settle interrupted work.
- struct `types::Extension` — Opaque adapter fields. The host replays them only to their originating plugin identity.
- struct `types::Failure` — An operation failure. `retryable` means the same request may succeed later.
- struct `types::Image` — An image in a message: the stored bytes, not base64.
- enum `types::ImageFormat` — Image encodings that every supported service accepts.
- struct `types::Message` — A canonical message sent to or returned by the model.
- enum `types::Outcome` — A known result, a known failure, or an external effect that cannot be confirmed. An unknown outcome must never be retried automatically.
- enum `types::Part` — Ordered text, images, tool interactions and opaque extensions within a message.
- enum `types::Role` — Identifies the speaker of a canonical model message.
- struct `types::Settings` — Endpoint, model and adapter options supplied by the caller for this call.
- struct `types::ToolCall` — A proposed tool invocation. `id` is the original service-assigned call ID, empty when the service assigns none.
- struct `types::ToolResult` — A tool settlement. `call-id` refers to the original `tool-call.id`.
- struct `types::ToolSpec` — A tool disclosed to the model.
- type `types::Json` — A JSON document encoded as UTF-8 text.

## enum `types::Effect`

`src/lib.rs`

```rust
pub enum Effect {
    ReadOnly,
    Idempotent,
    SideEffect,
}
```

Mutation semantics used to order calls and settle interrupted work.

Implements `Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`.

## struct `types::Extension`

`src/lib.rs`

```rust
pub struct Extension {
    pub data: Json,
}
```

Opaque adapter fields. The host replays them only to their originating plugin identity.

Implements `Clone`, `Debug`.

## struct `types::Failure`

`src/lib.rs`

```rust
pub struct Failure {
    pub code: _rt::String,
    pub message: _rt::String,
    pub retryable: bool,
}
```

An operation failure. `retryable` means the same request may succeed later.

Implements `Clone`, `Debug`, `Display`, `Error`.

## struct `types::Image`

`src/lib.rs`

```rust
pub struct Image {
    pub format: ImageFormat,
    pub data: _rt::Vec<u8>,
}
```

An image in a message: the stored bytes, not base64.

### `types::Image::base64`

```rust
pub fn base64(&self) -> String
```

The image bytes as standard Base64 with padding, as data URLs and inline JSON images expect.

Implements `Clone`, `Debug`.

## enum `types::ImageFormat`

`src/lib.rs`

```rust
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}
```

Image encodings that every supported service accepts.

### `types::ImageFormat::mime`

```rust
pub fn mime(self) -> &'static str
```

The MIME type of this format, such as `image/png`.

Implements `Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`.

## struct `types::Message`

`src/lib.rs`

```rust
pub struct Message {
    pub role: Role,
    pub parts: _rt::Vec<Part>,
}
```

A canonical message sent to or returned by the model.

Implements `Clone`, `Debug`.

## enum `types::Outcome`

`src/lib.rs`

```rust
pub enum Outcome {
    Ok(Json),
    Failed(Failure),
    Unknown(Failure),
}
```

A known result, a known failure, or an external effect that cannot be confirmed.
An unknown outcome must never be retried automatically.

Implements `Clone`, `Debug`.

## enum `types::Part`

`src/lib.rs`

```rust
pub enum Part {
    Text(_rt::String),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Extension(Extension),
    Image(Image),
}
```

Ordered text, images, tool interactions and opaque extensions within a message.

Implements `Clone`, `Debug`.

## enum `types::Role`

`src/lib.rs`

```rust
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}
```

Identifies the speaker of a canonical model message.

Implements `Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`.

## struct `types::Settings`

`src/lib.rs`

```rust
pub struct Settings {
    pub base_url: _rt::String,
    pub model: _rt::String,
    pub options: Json,
}
```

Endpoint, model and adapter options supplied by the caller for this call.

Implements `Clone`, `Debug`.

## struct `types::ToolCall`

`src/lib.rs`

```rust
pub struct ToolCall {
    pub id: _rt::String,
    pub name: _rt::String,
    pub arguments: _rt::String,
}
```

A proposed tool invocation. `id` is the original service-assigned call ID, empty when the service assigns none.

Implements `Clone`, `Debug`.

## struct `types::ToolResult`

`src/lib.rs`

```rust
pub struct ToolResult {
    pub call_id: _rt::String,
    pub content: _rt::String,
    pub is_error: bool,
}
```

A tool settlement. `call-id` refers to the original `tool-call.id`.

Implements `Clone`, `Debug`.

## struct `types::ToolSpec`

`src/lib.rs`

```rust
pub struct ToolSpec {
    pub name: _rt::String,
    pub description: _rt::String,
    pub input_schema: Json,
    pub effect: Effect,
}
```

A tool disclosed to the model.

Names use ASCII letters, digits, underscores or hyphens; tools.list names are unique within the plugin.
Completion adapters ignore effect; the host uses it when dispatching tools.

`input-schema` describes the argument object in a subset of JSON Schema:
- objects with `properties` and `required`, always with `additionalProperties: false`;
- strings with `enum`, `minLength` and `maxLength`;
- integers and numbers with `minimum` and `maximum`;
- booleans;
- arrays with `items`, `minItems` and `maxItems`;
- `anyOf` with two or more schemas.

Any schema below the root may carry a `description` and a `default` that fits it; the root
is an object and carries neither. A declaration outside the subset fails to load. The host
checks every call against the schema before the plugin sees it: an optional property given
as null counts as not given, and an integer written with a zero fraction, such as 5.0,
arrives as 5.

Implements `Clone`, `Debug`.

## type `types::Json`

`src/lib.rs`

```rust
pub type Json = _rt::String;
```

A JSON document encoded as UTF-8 text.

# module `enco_sdk::bindings::enco`

`src/lib.rs`

- module `bindings::enco::plugin` — (no summary)

# module `enco_sdk::bindings::exports`

`src/lib.rs`

- module `bindings::exports::enco` — (no summary)

# module `enco_sdk::bindings::enco::plugin`

`src/lib.rs`

- module `bindings::enco::plugin::host` — same as `host`
- module `bindings::enco::plugin::types` — same as `types`

# module `enco_sdk::bindings::exports::enco`

`src/lib.rs`

- module `bindings::exports::enco::plugin` — (no summary)

# module `enco_sdk::bindings::exports::enco::plugin`

`src/lib.rs`

- module `bindings::exports::enco::plugin::completion` — Adapts canonical requests to a model service protocol.
- module `bindings::exports::enco::plugin::lifecycle` — Describes and checks the component without assigning its registry identity.

# module `enco_sdk::bindings::exports::enco::plugin::completion`

`src/lib.rs`

Adapts canonical requests to a model service protocol.

- struct `bindings::exports::enco::plugin::completion::Completion` — same as `protocol::Completion`
- struct `bindings::exports::enco::plugin::completion::Request` — same as `protocol::Request`
- enum `bindings::exports::enco::plugin::completion::StopReason` — same as `protocol::StopReason`
- struct `bindings::exports::enco::plugin::completion::Usage` — same as `protocol::Usage`
- trait `bindings::exports::enco::plugin::completion::Guest` — (no summary)
- type `bindings::exports::enco::plugin::completion::Failure` — (no summary)
- type `bindings::exports::enco::plugin::completion::Message` — (no summary)
- type `bindings::exports::enco::plugin::completion::Settings` — (no summary)
- type `bindings::exports::enco::plugin::completion::ToolSpec` — (no summary)

## trait `bindings::exports::enco::plugin::completion::Guest`

`src/lib.rs`

```rust
pub trait Guest
```

### `bindings::exports::enco::plugin::completion::Guest::complete`

```rust
async fn complete(settings: Settings, request: Request) -> ::core::result::Result<Completion, Failure>
```

Perform one non-streaming completion.

#### Errors
Failure codes are `provider.network`, `provider.auth`, `provider.rate_limited`,
`provider.server`, `provider.bad_request`, `provider.bad_response`, and `timeout`.

## type `bindings::exports::enco::plugin::completion::Failure`

`src/lib.rs`

```rust
pub type Failure = super::super::super::super::enco::plugin::types::Failure;
```

## type `bindings::exports::enco::plugin::completion::Message`

`src/lib.rs`

```rust
pub type Message = super::super::super::super::enco::plugin::types::Message;
```

## type `bindings::exports::enco::plugin::completion::Settings`

`src/lib.rs`

```rust
pub type Settings = super::super::super::super::enco::plugin::types::Settings;
```

## type `bindings::exports::enco::plugin::completion::ToolSpec`

`src/lib.rs`

```rust
pub type ToolSpec = super::super::super::super::enco::plugin::types::ToolSpec;
```

# module `enco_sdk::bindings::exports::enco::plugin::lifecycle`

`src/lib.rs`

Describes and checks the component without assigning its registry identity.

- struct `bindings::exports::enco::plugin::lifecycle::Credential` — Named credential attached to `origin`, or to the caller-selected service when absent. A fixed origin uses the generation's secret mapping; an absent origin uses the endpoint's. Names and origins must be unique, including at most one absent origin. The host sets `header` to `prefix` followed by the credential value; plugins never see it.
- struct `bindings::exports::enco::plugin::lifecycle::Description` — Plugin-wide facts and requirements. `config-schema` documents configuration as JSON Schema; configuration validation belongs to `describe`, not to a second host-side schema validator.
- trait `bindings::exports::enco::plugin::lifecycle::Guest` — (no summary)
- type `bindings::exports::enco::plugin::lifecycle::Failure` — (no summary)
- type `bindings::exports::enco::plugin::lifecycle::Json` — (no summary)

## struct `bindings::exports::enco::plugin::lifecycle::Credential`

`src/lib.rs`

```rust
pub struct Credential {
    pub name: _rt::String,
    pub origin: ::core::option::Option<_rt::String>,
    pub header: _rt::String,
    pub prefix: _rt::String,
    pub required: bool,
}
```

Named credential attached to `origin`, or to the caller-selected service when absent.
A fixed origin uses the generation's secret mapping; an absent origin uses the endpoint's.
Names and origins must be unique, including at most one absent origin.
The host sets `header` to `prefix` followed by the credential value; plugins never see it.

Implements `Clone`, `Debug`.

## struct `bindings::exports::enco::plugin::lifecycle::Description`

`src/lib.rs`

```rust
pub struct Description {
    pub summary: _rt::String,
    pub config_schema: Json,
    pub credentials: _rt::Vec<Credential>,
}
```

Plugin-wide facts and requirements. `config-schema` documents configuration as JSON Schema;
configuration validation belongs to `describe`, not to a second host-side schema validator.

Implements `Clone`, `Debug`.

## trait `bindings::exports::enco::plugin::lifecycle::Guest`

`src/lib.rs`

```rust
pub trait Guest
```

### `bindings::exports::enco::plugin::lifecycle::Guest::describe`

```rust
fn describe(config: Json) -> ::core::result::Result<Description, Failure>
```

Describe the component or reject its configuration. An empty object must be accepted
and provide the plugin's minimum capabilities. Declarations are fixed with the generation.

### `bindings::exports::enco::plugin::lifecycle::Guest::probe`

```rust
async fn probe() -> ::core::result::Result<(), Failure>
```

Check the component without network access or owner-specific configuration.

## type `bindings::exports::enco::plugin::lifecycle::Failure`

`src/lib.rs`

```rust
pub type Failure = super::super::super::super::enco::plugin::types::Failure;
```

## type `bindings::exports::enco::plugin::lifecycle::Json`

`src/lib.rs`

```rust
pub type Json = super::super::super::super::enco::plugin::types::Json;
```
