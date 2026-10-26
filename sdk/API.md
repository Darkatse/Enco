<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_sdk`

`src/lib.rs`

Rust bindings, world exports, and shared schema and protocol helpers for plugins.

The host writes this crate to `sdk/` in every home, replacing any edits there; plugins depend on
it by path.

- module `bindings` — Types and host functions generated from the WIT.
- module `host` — Host-owned logging and transport available to plugins.
- module `protocol` — Helpers for plugins that call model services through the host's `http` import, including OpenAI-compatible Chat Completions and embeddings.
- module `types` — Canonical data exchanged between the host and plugins.
- macro `export` — Export `$implementation` as the plugin for one world: `completion`, `provider` (completion and embedding), `decision`, `tools`, `composer` or `channel`.
- fn `input_schema` — The canonical schema of `T`, used by tool arguments and composer options.
- fn `parse_arguments` — Parse a tool call's `input` into `T`; a failure has code `tool.invalid_arguments` and is not retryable.
- fn `parse_options` — Parse normalized composer options into the type that generated their schema.
- fn `source_hash` — The lowercase BLAKE3 digest of source text before the composer formats it.

Re-exports `wit_bindgen`.

## macro `export`

`src/lib.rs`

```rust
macro_rules! export {
    (completion, $implementation:ident) => { ... };
    (provider, $implementation:ident) => { ... };
    (decision, $implementation:ident) => { ... };
    (tools, $implementation:ident) => { ... };
    (composer, $implementation:ident) => { ... };
    (channel, $implementation:ident) => { ... };
    (@provider $world:literal, $implementation:ident) => { ... };
    (@world $world:literal, $implementation:ident, [$($derives:path),*], $($types:tt)*) => { ... };
    (@bindings $world:literal, $implementation:ident, [$($derives:path),*], $($types:tt)*) => { ... };
}
```

Export `$implementation` as the plugin for one world: `completion`, `provider` (completion and
embedding), `decision`, `tools`, `composer` or `channel`.

Call it in a crate in `plugins/<name>` that depends on this SDK as `enco_sdk`. It defines a
`bindings` module beside the call, whose `exports` hold the `Guest` traits to implement; the
WIT is read from `../../sdk/wit`.

## fn `input_schema`

`src/lib.rs`

```rust
pub fn input_schema<T: schemars::JsonSchema>() -> serde_json::Value
```

The canonical schema of `T`, used by tool arguments and composer options.

Return it as JSON text from a tool's `types::ToolSpec` or a composer's `options-schema`.
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

## fn `parse_options`

`src/lib.rs`

```rust
pub fn parse_options<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, types::Failure>
```

Parse normalized composer options into the type that generated their schema.

The host has already checked the options; custom deserialization failures are `compose.failed`.

## fn `source_hash`

`src/lib.rs`

```rust
pub fn source_hash(text: &str) -> String
```

The lowercase BLAKE3 digest of source text before the composer formats it.

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
- struct `protocol::Completion` — A completed model attempt.
- struct `protocol::Request` — The frozen messages, disclosed tools and output budget for one completion.
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

A completed model attempt.

Canonical parts are text, tool calls and extensions; generated images stay in extensions.

- `message` — The reply, with the `assistant` role.
- `usage` — What the attempt cost in tokens.
- `stop_reason` — Why generation stopped.

Implements:

```rust
impl Clone for Completion
impl Debug for Completion
impl PartialEq for Completion
```

## struct `protocol::Request`

`src/lib.rs`

```rust
pub struct Request {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_output_tokens: ::core::option::Option<u32>,
}
```

The frozen messages, disclosed tools and output budget for one completion.

User messages may include images; tool messages contain one result followed by any images.

- `messages` — The messages, in order.
- `tools` — The tools the model may call.
- `max_output_tokens` — The most tokens the reply may use; the service's default when absent.

Implements:

```rust
impl Clone for Request
impl Debug for Request
impl PartialEq for Request
```

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

Implements:

```rust
impl Clone for StopReason
impl Copy for StopReason
impl Debug for StopReason
impl Eq for StopReason
impl Ord for StopReason
impl PartialEq for StopReason
impl PartialOrd for StopReason
```

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

- `input_tokens` — Tokens the request used.
- `output_tokens` — Tokens the reply used.
- `cached_input_tokens` — The part of `input-tokens` served from the service's cache, when it reports one.

Implements:

```rust
impl Clone for Usage
impl Copy for Usage
impl Debug for Usage
impl PartialEq for Usage
```

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
- struct `host::HttpFailure` — Transport failure.
- struct `host::HttpRequest` — An HTTP request for the host to send.
- struct `host::HttpResponse` — An HTTP status, headers and fully collected response body.
- enum `host::LogLevel` — Severity attached to a plugin diagnostic.
- fn `host::config` — The configuration of this invocation's generation, excluding credential mappings.
- fn `host::http` — Send a request and collect its response; non-2xx statuses are returned as responses.
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

Implements:

```rust
impl Clone for HttpErrorKind
impl Copy for HttpErrorKind
impl Debug for HttpErrorKind
impl Eq for HttpErrorKind
impl Ord for HttpErrorKind
impl PartialEq for HttpErrorKind
impl PartialOrd for HttpErrorKind
```

## struct `host::HttpFailure`

`src/lib.rs`

```rust
pub struct HttpFailure {
    pub kind: HttpErrorKind,
    pub message: String,
    pub request_sent: bool,
}
```

Transport failure.

- `kind` — The stage that failed.
- `message` — What went wrong.
- `request_sent` — Whether the request may have reached the peer.

Implements:

```rust
impl Clone for HttpFailure
impl Debug for HttpFailure
impl Display for HttpFailure
impl Error for HttpFailure
impl PartialEq for HttpFailure
```

## struct `host::HttpRequest`

`src/lib.rs`

```rust
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: ::core::option::Option<Vec<u8>>,
    pub timeout_ms: ::core::option::Option<u32>,
}
```

An HTTP request for the host to send.

- `method` — The method, such as `GET` or `POST`.
- `url` — The absolute URL.
- `headers` — Header names and values, in order.
- `body` — The bytes to send, if any.
- `timeout_ms` — The time limit in milliseconds; the host's default when absent.

Implements:

```rust
impl Clone for HttpRequest
impl Debug for HttpRequest
impl PartialEq for HttpRequest
```

## struct `host::HttpResponse`

`src/lib.rs`

```rust
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
```

An HTTP status, headers and fully collected response body.

- `status` — The status code, such as 200.
- `headers` — Header names and values, in order.
- `body` — The whole body, already collected.

Implements:

```rust
impl Clone for HttpResponse
impl Debug for HttpResponse
impl PartialEq for HttpResponse
```

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

Implements:

```rust
impl Clone for LogLevel
impl Copy for LogLevel
impl Debug for LogLevel
impl Eq for LogLevel
impl Ord for LogLevel
impl PartialEq for LogLevel
impl PartialOrd for LogLevel
```

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

Send a request and collect its response; non-2xx statuses are returned as responses.

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
- struct `types::Failure` — An operation failure.
- struct `types::Image` — An image in a message.
- enum `types::ImageFormat` — Image encodings that every supported service accepts.
- struct `types::Message` — A canonical message sent to or returned by the model.
- enum `types::Outcome` — A known result, a known failure, or an external effect that cannot be confirmed.
- enum `types::Part` — Ordered text, images, tool interactions and opaque extensions within a message.
- enum `types::Role` — Identifies the speaker of a canonical model message.
- struct `types::Settings` — Endpoint, model and adapter options supplied by the caller for this call.
- struct `types::ToolCall` — A proposed tool invocation.
- struct `types::ToolResult` — A tool settlement.
- struct `types::ToolSpec` — A tool disclosed to the model.
- struct `types::Unviewable` — An attachment kept only as a description.
- enum `types::UnviewableReason` — Why an attachment did not become an image.
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

Consecutive read-only calls run together; other calls run one at a time. A call cut off by
a crash fails as retryable when it is read-only, and settles as unknown otherwise.

- `ReadOnly` — Changes nothing beyond its result.
- `Idempotent` — Making the call again has the same effect as making it once.
- `SideEffect` — Changes something that must not be changed again blindly.

Implements:

```rust
impl Clone for Effect
impl Copy for Effect
impl Debug for Effect
impl Eq for Effect
impl Ord for Effect
impl PartialEq for Effect
impl PartialOrd for Effect
```

## struct `types::Extension`

`src/lib.rs`

```rust
pub struct Extension {
    pub data: Json,
}
```

Opaque adapter fields. The host replays them only to their originating plugin identity.

- `data` — The fields, as the adapter that wrote them expects them back.

Implements:

```rust
impl Clone for Extension
impl Debug for Extension
impl PartialEq for Extension
```

## struct `types::Failure`

`src/lib.rs`

```rust
pub struct Failure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
```

An operation failure.

- `code` — A stable classification, such as `timeout` or `provider.auth`.
- `message` — What went wrong, for the Agent and the owner to read.
- `retryable` — Whether the same request may succeed later.

Implements:

```rust
impl Clone for Failure
impl Debug for Failure
impl Display for Failure
impl Error for Failure
impl PartialEq for Failure
```

## struct `types::Image`

`src/lib.rs`

```rust
pub struct Image {
    pub format: ImageFormat,
    pub data: Vec<u8>,
}
```

An image in a message.

- `format` — How `data` is encoded.
- `data` — The image bytes as stored, not base64.

### `types::Image::base64`

```rust
pub fn base64(&self) -> String
```

The image bytes as standard Base64 with padding, as data URLs and inline JSON images expect.

Implements:

```rust
impl Clone for Image
impl Debug for Image
impl PartialEq for Image
```

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

Implements:

```rust
impl Clone for ImageFormat
impl Copy for ImageFormat
impl Debug for ImageFormat
impl Eq for ImageFormat
impl Ord for ImageFormat
impl PartialEq for ImageFormat
impl PartialOrd for ImageFormat
```

## struct `types::Message`

`src/lib.rs`

```rust
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}
```

A canonical message sent to or returned by the model.

- `role` — Who speaks.
- `parts` — The content, in order.

Implements:

```rust
impl Clone for Message
impl Debug for Message
impl PartialEq for Message
```

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

- `Ok` — The call succeeded with this JSON result.
- `Failed` — The call failed, and its effects are known.
- `Unknown` — An external effect may have happened and cannot be confirmed; it is never retried
  automatically.

Implements:

```rust
impl Clone for Outcome
impl Debug for Outcome
impl PartialEq for Outcome
```

## enum `types::Part`

`src/lib.rs`

```rust
pub enum Part {
    Text(String),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Extension(Extension),
    Image(Image),
}
```

Ordered text, images, tool interactions and opaque extensions within a message.

- `Text` — Text the speaker wrote.
- `ToolCall` — A tool call the assistant proposes.
- `ToolResult` — The result of a tool call.
- `Extension` — Adapter fields kept for the plugin that wrote them.
- `Image` — An image attached to the message.

Implements:

```rust
impl Clone for Part
impl Debug for Part
impl PartialEq for Part
```

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

Implements:

```rust
impl Clone for Role
impl Copy for Role
impl Debug for Role
impl Eq for Role
impl Ord for Role
impl PartialEq for Role
impl PartialOrd for Role
```

## struct `types::Settings`

`src/lib.rs`

```rust
pub struct Settings {
    pub base_url: String,
    pub model: String,
    pub options: Json,
}
```

Endpoint, model and adapter options supplied by the caller for this call.

- `base_url` — The service's base URL; request paths are appended to it.
- `model` — The model the service runs.
- `options` — Options the adapter merges into its request, as a JSON object.

Implements:

```rust
impl Clone for Settings
impl Debug for Settings
impl PartialEq for Settings
```

## struct `types::ToolCall`

`src/lib.rs`

```rust
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}
```

A proposed tool invocation.

- `id` — The call ID the service assigned, or empty when it assigns none.
- `name` — The tool to call, by the name its `tool-spec` declares.
- `arguments` — The arguments as the model wrote them, as JSON text.

Implements:

```rust
impl Clone for ToolCall
impl Debug for ToolCall
impl PartialEq for ToolCall
```

## struct `types::ToolResult`

`src/lib.rs`

```rust
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
}
```

A tool settlement.

- `call_id` — The `id` of the `tool-call` this result answers.
- `content` — The result text the model reads.
- `is_error` — Whether the call failed.

Implements:

```rust
impl Clone for ToolResult
impl Debug for ToolResult
impl PartialEq for ToolResult
```

## struct `types::ToolSpec`

`src/lib.rs`

```rust
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Json,
    pub effect: Effect,
}
```

A tool disclosed to the model.

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

- `name` — The name the model calls the tool by: ASCII letters, digits, underscores or hyphens,
  unique among the plugin's tools.
- `description` — What the tool does, for the model to read.
- `input_schema` — The argument object, in the subset of JSON Schema stated above.
- `effect` — What a call may change; completion adapters ignore it, and the host uses it when
  dispatching.

Implements:

```rust
impl Clone for ToolSpec
impl Debug for ToolSpec
impl PartialEq for ToolSpec
```

## struct `types::Unviewable`

`src/lib.rs`

```rust
pub struct Unviewable {
    pub what: String,
    pub reason: UnviewableReason,
}
```

An attachment kept only as a description.

- `what` — The channel's description of it.
- `reason` — Why it is not an image.

Implements:

```rust
impl Clone for Unviewable
impl Debug for Unviewable
impl PartialEq for Unviewable
```

## enum `types::UnviewableReason`

`src/lib.rs`

```rust
pub enum UnviewableReason {
    Unsupported,
    TooLarge,
    Unavailable(String),
}
```

Why an attachment did not become an image.

- `Unsupported` — Its format is not an accepted image format.
- `TooLarge` — It is larger than an image may be.
- `Unavailable` — It could not be retrieved, for this reason.

Implements:

```rust
impl Clone for UnviewableReason
impl Debug for UnviewableReason
impl PartialEq for UnviewableReason
```

## type `types::Json`

`src/lib.rs`

```rust
pub type Json = String;
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

- struct `bindings::exports::enco::plugin::lifecycle::Credential` — Named credential attached to `origin`, or to the caller-selected service when absent.
- struct `bindings::exports::enco::plugin::lifecycle::Description` — Plugin-wide facts and requirements.
- struct `bindings::exports::enco::plugin::lifecycle::HeaderPlacement` — A request header that carries a credential.
- enum `bindings::exports::enco::plugin::lifecycle::Placement` — Where the host writes a credential; the plugin never sees the value.
- trait `bindings::exports::enco::plugin::lifecycle::Guest` — (no summary)
- type `bindings::exports::enco::plugin::lifecycle::Failure` — (no summary)
- type `bindings::exports::enco::plugin::lifecycle::Json` — (no summary)

## struct `bindings::exports::enco::plugin::lifecycle::Credential`

`src/lib.rs`

```rust
pub struct Credential {
    pub name: String,
    pub origin: ::core::option::Option<String>,
    pub placement: Placement,
    pub required: bool,
}
```

Named credential attached to `origin`, or to the caller-selected service when absent.

A fixed origin uses the generation's secret mapping; an absent origin uses the endpoint's.
Names and origins must be unique, including at most one absent origin.
The host writes the value at `placement`; plugins never see it.

- `name` — The name the owner's configuration maps to an environment variable.
- `origin` — The origin the credential is sent to; absent for the service the caller selects.
- `placement` — Where the host writes the credential.
- `required` — Whether the plugin cannot work without it.

Implements:

```rust
impl Clone for Credential
impl Debug for Credential
impl PartialEq for Credential
```

## struct `bindings::exports::enco::plugin::lifecycle::Description`

`src/lib.rs`

```rust
pub struct Description {
    pub summary: String,
    pub config_schema: Json,
    pub credentials: Vec<Credential>,
}
```

Plugin-wide facts and requirements.

- `summary` — One line on what the plugin offers.
- `config_schema` — The configuration the plugin accepts, as JSON Schema; only `describe` validates it.
- `credentials` — The credentials the plugin uses.

Implements:

```rust
impl Clone for Description
impl Debug for Description
impl PartialEq for Description
```

## struct `bindings::exports::enco::plugin::lifecycle::HeaderPlacement`

`src/lib.rs`

```rust
pub struct HeaderPlacement {
    pub name: String,
    pub prefix: String,
}
```

A request header that carries a credential.

- `name` — The header's name.
- `prefix` — Text placed before the value, such as `Bearer `.

Implements:

```rust
impl Clone for HeaderPlacement
impl Debug for HeaderPlacement
impl PartialEq for HeaderPlacement
```

## enum `bindings::exports::enco::plugin::lifecycle::Placement`

`src/lib.rs`

```rust
pub enum Placement {
    Header(HeaderPlacement),
    Path(Vec<String>),
}
```

Where the host writes a credential; the plugin never sees the value.

- `Header` — A header set to its prefix followed by the value.
- `Path` — Insert after the matching prefix at the beginning of the request path.

Implements:

```rust
impl Clone for Placement
impl Debug for Placement
impl PartialEq for Placement
```

## trait `bindings::exports::enco::plugin::lifecycle::Guest`

`src/lib.rs`

```rust
pub trait Guest
```

### `bindings::exports::enco::plugin::lifecycle::Guest::describe`

```rust
fn describe(config: Json) -> ::core::result::Result<Description, Failure>
```

Describe the component or reject its configuration.

An empty object must be accepted and provide the plugin's minimum capabilities.
Declarations are fixed with the generation, so this runs without network access.

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
