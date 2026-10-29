<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_sdk`

`src/lib.rs`

Rust bindings, world exports, and shared schema, protocol and channel helpers for plugins.

The host writes this crate to `sdk/` in every home, replacing any edits there; plugins depend on
it by path.

- module `bindings` — Types and host functions generated from the WIT.
- module `channel` — What every channel mapper shares: commands, the activity card's wording and `multipart/form-data` uploads.
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
WIT is read from `../../sdk/wit`. The traits take the SDK's own types where it has helpers for
them, such as `protocol::Request`, `channel::Action` and `channel::CommandKind`; name
those from the SDK, not from `bindings`.

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

Plugins built with `export!` use these same types, so values pass between a plugin and
`protocol` or `channel` as they are.

- module `bindings::enco` — (no summary)
- module `bindings::exports` — (no summary)

# module `enco_sdk::channel`

`src/channel.rs`

What every channel mapper shares: commands, the activity card's wording and `multipart/form-data` uploads.

Each protocol marks commands and lays the card out its own way; the names, meanings and words
are the same everywhere. `export!` gives a channel plugin these same types, so
name them from here.

- enum `channel::Action` — What an incoming message asks for, whatever the protocol's command syntax.
- struct `channel::ActivityView` — What the Agent is doing while a Run goes on.
- struct `channel::Attachment` — An image attachment described by its protocol locator.
- enum `channel::CommandKind` — The commands the host understands.
- struct `channel::Content` — The text and attachments of a message.
- struct `channel::FormPart` — One part of a `multipart/form-data` body.
- struct `channel::ToolActivity` — One tool call shown in an activity view.
- enum `channel::ToolState` — Where a tool call stands.
- fn `channel::multipart` — Encode `parts` in order as `multipart/form-data`, returning the `content-type` header value and the body.
- fn `channel::typed_command` — The command typed at the start of `text`, as its name and the rest as its argument; absent when `text` starts otherwise.
- const `channel::CANCEL_LABEL` — The label of the activity card's button that cancels the Run.

## enum `channel::Action`

`src/lib.rs`

```rust
pub enum Action {
    Message(Content),
    Sessions,
    SelectSession(String),
    Cancel,
    Backtrack,
    Everyone(::core::option::Option<bool>),
}
```

What an incoming message asks for, whatever the protocol's command syntax.

- `Message` — A message for the conversation's Session.
- `Sessions` — List the Sessions to choose from.
- `SelectSession` — Route this conversation to the named Session.
- `Cancel` — Stop the work the conversation's Session is doing.
- `Backtrack` — Take the conversation back to the message `reply-to` names.
- `Everyone` — Open this group to everyone or close it to the owner only; absent reports which.

Implements:

```rust
impl Clone for Action
impl Debug for Action
impl PartialEq for Action
```

## struct `channel::ActivityView`

`src/lib.rs`

```rust
pub struct ActivityView {
    pub elapsed_seconds: u32,
    pub tools: Vec<ToolActivity>,
}
```

What the Agent is doing while a Run goes on.

- `elapsed_seconds` — Seconds since the Run started.
- `tools` — The Run's latest tool calls, oldest first.

### `channel::ActivityView::text`

`src/channel.rs`

```rust
pub fn text(&self) -> String
```

The activity card's text: `Working` and the elapsed `m:ss`, then a line per tool.

Each tool line is a mark, the tool's name and, after ` · `, its argument summary when it
has one; the marks are `…` running, `✓` ok, `✗` failed and `?` unknown.

Implements:

```rust
impl Clone for ActivityView
impl Debug for ActivityView
impl PartialEq for ActivityView
```

## struct `channel::Attachment`

`src/lib.rs`

```rust
pub struct Attachment {
    pub what: String,
    pub locator: String,
}
```

An image attachment described by its protocol locator.

- `what` — What the attachment is, for a message that cannot show it.
- `locator` — The opaque locator passed to `fetch`.

Implements:

```rust
impl Clone for Attachment
impl Debug for Attachment
impl PartialEq for Attachment
```

## enum `channel::CommandKind`

`src/lib.rs`

```rust
pub enum CommandKind {
    Session,
    Cancel,
    Backtrack,
    Everyone,
}
```

The commands the host understands.

- `Session` — List the Sessions, or switch to the one named after it.
- `Cancel` — Stop the work of the conversation's Session.
- `Backtrack` — Go back to the message the command replies to.
- `Everyone` — Open the group to everyone, or close it to the owner only.

### `channel::CommandKind::name`

`src/channel.rs`

```rust
pub fn name(self) -> &'static str
```

The shared name, without a leading `/` or a protocol's decoration.

### `channel::CommandKind::from_name`

`src/channel.rs`

```rust
pub fn from_name(name: &str) -> Option<Self>
```

The command with this exact, case-sensitive name; unknown names are ordinary text.

### `channel::CommandKind::action`

`src/channel.rs`

```rust
pub fn action(self, argument: &str) -> Option<Action>
```

What the command asks for with `argument`, compared without surrounding whitespace;
unknown arguments are ordinary text.

Implements:

```rust
impl Clone for CommandKind
impl Copy for CommandKind
impl Debug for CommandKind
impl Eq for CommandKind
impl Ord for CommandKind
impl PartialEq for CommandKind
impl PartialOrd for CommandKind
```

## struct `channel::Content`

`src/lib.rs`

```rust
pub struct Content {
    pub text: String,
    pub images: Vec<Attachment>,
    pub unviewable: Vec<Unviewable>,
}
```

The text and attachments of a message.

- `text` — What the sender wrote.
- `images` — Images, by the protocol's locator for fetching them.
- `unviewable` — Attachments that cannot be images, and why.

Implements:

```rust
impl Clone for Content
impl Debug for Content
impl PartialEq for Content
```

## struct `channel::FormPart`

`src/channel.rs`

```rust
pub struct FormPart<'a> {
    pub name: &'a str,
    pub filename: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub data: &'a [u8],
}
```

One part of a `multipart/form-data` body.

- `name` — The form field's name.
- `filename` — The uploaded file's name, sent with `\r`, `\n` and `"` percent-encoded; absent for a field
  that is not a file.
- `content_type` — The part's `Content-Type`; absent leaves the receiver's default, `text/plain`.
- `data` — The part's bytes, sent as they are.

## struct `channel::ToolActivity`

`src/lib.rs`

```rust
pub struct ToolActivity {
    pub name: String,
    pub state: ToolState,
    pub summary: String,
}
```

One tool call shown in an activity view.

- `name` — The tool's name, as the model calls it.
- `state` — Where the call stands.
- `summary` — The values of the call's required arguments in its schema's order, on one line the host
  shortens; empty when it has none.

Implements:

```rust
impl Clone for ToolActivity
impl Debug for ToolActivity
impl PartialEq for ToolActivity
```

## enum `channel::ToolState`

`src/lib.rs`

```rust
pub enum ToolState {
    Running,
    Ok,
    Failed,
    Unknown,
}
```

Where a tool call stands.

- `Running` — The call has started and has no settlement yet.
- `Ok` — It succeeded.
- `Failed` — The call finished with a known failure.
- `Unknown` — It may have taken effect, but its result is not known.

Implements:

```rust
impl Clone for ToolState
impl Copy for ToolState
impl Debug for ToolState
impl Eq for ToolState
impl Ord for ToolState
impl PartialEq for ToolState
impl PartialOrd for ToolState
```

## fn `channel::multipart`

`src/channel.rs`

```rust
pub fn multipart(parts: &[FormPart<'_>]) -> (String, Vec<u8>)
```

Encode `parts` in order as `multipart/form-data`, returning the `content-type` header value and
the body.

The boundary occurs in no part's name, file name or bytes.

## fn `channel::typed_command`

`src/channel.rs`

```rust
pub fn typed_command(text: &str) -> Option<(&str, &str)>
```

The command typed at the start of `text`, as its name and the rest as its argument; absent
when `text` starts otherwise.

The name follows a half-width `/` up to the first whitespace and keeps any decoration the
protocol adds, such as Telegram's `@bot`.

## const `channel::CANCEL_LABEL`

`src/channel.rs`

```rust
pub const CANCEL_LABEL: &str = "Cancel";
```

The label of the activity card's button that cancels the Run.

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

- module `bindings::exports::enco::plugin::channel` — Translates a messaging protocol as a pure state machine; the host owns connections, timers and credentials.
- module `bindings::exports::enco::plugin::completion` — Adapts canonical requests to a model service protocol.
- module `bindings::exports::enco::plugin::lifecycle` — Describes and checks the component without assigning its registry identity.

# module `enco_sdk::bindings::exports::enco::plugin::channel`

`src/lib.rs`

Translates a messaging protocol as a pure state machine; the host owns connections, timers and credentials.

The host records each external input before handing it over, and carries out what the mapper asks.

- enum `bindings::exports::enco::plugin::channel::Action` — same as `channel::Action`
- struct `bindings::exports::enco::plugin::channel::ActivityView` — same as `channel::ActivityView`
- enum `bindings::exports::enco::plugin::channel::Arrival` — Something that reached the mapper through the host.
- struct `bindings::exports::enco::plugin::channel::Attachment` — same as `channel::Attachment`
- struct `bindings::exports::enco::plugin::channel::Closure` — How a socket ended.
- struct `bindings::exports::enco::plugin::channel::Command` — A command the host understands, as it should appear in the protocol's menus.
- enum `bindings::exports::enco::plugin::channel::CommandKind` — same as `channel::CommandKind`
- struct `bindings::exports::enco::plugin::channel::Content` — same as `channel::Content`
- struct `bindings::exports::enco::plugin::channel::Fault` — What a protocol answer means for the host's settlement and retries.
- enum `bindings::exports::enco::plugin::channel::FetchStep` — The next step of fetching an attachment.
- struct `bindings::exports::enco::plugin::channel::FileData` — File bytes for one outgoing message.
- struct `bindings::exports::enco::plugin::channel::Identity` — The account a connection reaches.
- struct `bindings::exports::enco::plugin::channel::Inbound` — A message that arrived, as the protocol describes it.
- struct `bindings::exports::enco::plugin::channel::Outgoing` — Markdown text with at most one file to send.
- struct `bindings::exports::enco::plugin::channel::PacingGap` — The time the host leaves before each message it sends through one connection.
- enum `bindings::exports::enco::plugin::channel::ReadStep` — How the host reads the protocol next.
- struct `bindings::exports::enco::plugin::channel::Reading` — What the mapper asks of the host after one arrival.
- struct `bindings::exports::enco::plugin::channel::Request` — A request for the host to send.
- struct `bindings::exports::enco::plugin::channel::Response` — A response the protocol returned, whatever its status.
- struct `bindings::exports::enco::plugin::channel::Socket` — A WebSocket the host opens and keeps until the mapper asks to reconnect or the connection stops.
- struct `bindings::exports::enco::plugin::channel::ToolActivity` — same as `channel::ToolActivity`
- enum `bindings::exports::enco::plugin::channel::ToolState` — same as `channel::ToolState`
- trait `bindings::exports::enco::plugin::channel::Guest` — (no summary)
- type `bindings::exports::enco::plugin::channel::Failure` — (no summary)
- type `bindings::exports::enco::plugin::channel::Json` — (no summary)
- type `bindings::exports::enco::plugin::channel::Unviewable` — (no summary)

## enum `bindings::exports::enco::plugin::channel::Arrival`

`src/lib.rs`

```rust
pub enum Arrival {
    Response(Response),
    Frame(String),
    Closed(Closure),
    Timer,
}
```

Something that reached the mapper through the host.

- `Response` — The response to the read request.
- `Frame` — A text frame on the open socket.
- `Closed` — The socket closed, by the protocol or through a network failure.
- `Timer` — The timer the mapper set has expired; its reading can change state and request actions, but has no inbound messages.

Implements:

```rust
impl Clone for Arrival
impl Debug for Arrival
impl PartialEq for Arrival
```

## struct `bindings::exports::enco::plugin::channel::Closure`

`src/lib.rs`

```rust
pub struct Closure {
    pub code: ::core::option::Option<u16>,
    pub reason: String,
}
```

How a socket ended.

- `code` — The close code, when the protocol sent one.
- `reason` — The close reason, or the host's description of the network failure.

Implements:

```rust
impl Clone for Closure
impl Debug for Closure
impl PartialEq for Closure
```

## struct `bindings::exports::enco::plugin::channel::Command`

`src/lib.rs`

```rust
pub struct Command {
    pub kind: CommandKind,
    pub description: String,
    pub direct: bool,
    pub group: bool,
}
```

A command the host understands, as it should appear in the protocol's menus.

- `kind` — Which command.
- `description` — What it does, for the menu.
- `direct` — Whether it applies in one-to-one conversations.
- `group` — Whether it applies in group conversations.

Implements:

```rust
impl Clone for Command
impl Debug for Command
impl PartialEq for Command
```

## struct `bindings::exports::enco::plugin::channel::Fault`

`src/lib.rs`

```rust
pub struct Fault {
    pub failure: Failure,
    pub unknown: bool,
    pub fatal: bool,
    pub retry_after: ::core::option::Option<u32>,
}
```

What a protocol answer means for the host's settlement and retries.

- `failure` — What failed.
- `unknown` — The request may have taken effect, so it must not be sent again.
- `fatal` — The whole connection is unusable, not just this request or destination.
- `retry_after` — How long the protocol asks to wait before retrying, in seconds.

Implements:

```rust
impl Clone for Fault
impl Debug for Fault
impl Display for Fault
impl Error for Fault
impl PartialEq for Fault
```

## enum `bindings::exports::enco::plugin::channel::FetchStep`

`src/lib.rs`

```rust
pub enum FetchStep {
    Request(Request),
    Download(String),
    Image(Vec<u8>),
    Unavailable(String),
}
```

The next step of fetching an attachment.

- `Request` — Send this request and call again with its response.
- `Download` — Get this absolute URL taken from the protocol's content and call again with its
  response.

  The host sends it without credentials and only to one of the mapper's `origins`, so
  content never chooses a path on the service; an address elsewhere makes the attachment
  unavailable.
- `Image` — The attachment's bytes.
- `Unavailable` — The protocol refused it for good; the text says why.

Implements:

```rust
impl Clone for FetchStep
impl Debug for FetchStep
impl PartialEq for FetchStep
```

## struct `bindings::exports::enco::plugin::channel::FileData`

`src/lib.rs`

```rust
pub struct FileData {
    pub name: String,
    pub data: Vec<u8>,
}
```

File bytes for one outgoing message.

- `name` — The filename without its original directory.
- `data` — The original bytes, encoded by the mapper for transport.

Implements:

```rust
impl Clone for FileData
impl Debug for FileData
impl PartialEq for FileData
```

## struct `bindings::exports::enco::plugin::channel::Identity`

`src/lib.rs`

```rust
pub struct Identity {
    pub protocol: String,
    pub account: String,
    pub facts: Json,
}
```

The account a connection reaches.

- `protocol` — A nonempty protocol name without colons; with `account`, it keys connection state.
- `account` — A nonempty, stable account ID within the protocol, treated as opaque by the host.
- `facts` — What the mapper needs to know about the account later, such as the bot's username.

Implements:

```rust
impl Clone for Identity
impl Debug for Identity
impl PartialEq for Identity
```

## struct `bindings::exports::enco::plugin::channel::Inbound`

`src/lib.rs`

```rust
pub struct Inbound {
    pub key: String,
    pub conversation: String,
    pub message: String,
    pub sender: ::core::option::Option<String>,
    pub sender_name: ::core::option::Option<String>,
    pub direct: bool,
    pub addressed: bool,
    pub reply_to: ::core::option::Option<String>,
    pub respond: ::core::option::Option<String>,
    pub action: Action,
}
```

A message that arrived, as the protocol describes it.

- `key` — The protocol's key for accepting this message once, such as an update or event ID.
- `conversation` — The conversation, where replies go.
- `message` — This message's ID, unique within the conversation.
- `sender` — The sender's ID, when the protocol reports one.
- `sender_name` — The sender's display name, when the protocol reports one.
- `direct` — Whether the conversation is one to one.
- `addressed` — Whether the message is addressed to this bot; always true in a direct conversation.
- `reply_to` — The message this one replies to, or the message a command was invoked on.
- `respond` — The handle for answering this message once accepted, when the protocol expects an
  answer, such as for a command or a button.
- `action` — What it asks for.

Implements:

```rust
impl Clone for Inbound
impl Debug for Inbound
impl PartialEq for Inbound
```

## struct `bindings::exports::enco::plugin::channel::Outgoing`

`src/lib.rs`

```rust
pub struct Outgoing {
    pub text: String,
    pub file: ::core::option::Option<FileData>,
}
```

Markdown text with at most one file to send.

- `text` — All text to send, in Markdown.
- `file` — A file to send with this text, if any.

Implements:

```rust
impl Clone for Outgoing
impl Debug for Outgoing
impl PartialEq for Outgoing
```

## struct `bindings::exports::enco::plugin::channel::PacingGap`

`src/lib.rs`

```rust
pub struct PacingGap {
    pub minimum_ms: u32,
    pub jitter_ms: u32,
}
```

The time the host leaves before each message it sends through one connection.

Each field is at most the limit the manual lists for channel `pacing`; a larger gap fails
loading, because stopping a connection waits out a started gap.

- `minimum_ms` — The least milliseconds from one message finishing to the next one starting.
- `jitter_ms` — Up to this many more milliseconds before each message, chosen at random.

Implements:

```rust
impl Clone for PacingGap
impl Copy for PacingGap
impl Debug for PacingGap
impl PartialEq for PacingGap
```

## enum `bindings::exports::enco::plugin::channel::ReadStep`

`src/lib.rs`

```rust
pub enum ReadStep {
    Request(Request),
    Socket(Socket),
}
```

How the host reads the protocol next.

- `Request` — Send this request; its response is one arrival.
- `Socket` — Open this socket; each frame on it is one arrival, until it closes.

Implements:

```rust
impl Clone for ReadStep
impl Debug for ReadStep
impl PartialEq for ReadStep
```

## struct `bindings::exports::enco::plugin::channel::Reading`

`src/lib.rs`

```rust
pub struct Reading {
    pub messages: Vec<Inbound>,
    pub state: ::core::option::Option<Json>,
    pub frames: Vec<String>,
    pub acknowledgments: Vec<Request>,
    pub timer: ::core::option::Option<u32>,
    pub reconnect: bool,
}
```

What the mapper asks of the host after one arrival.

- `messages` — Messages to accept, in order.
- `state` — The protocol state after this arrival; absent leaves it unchanged.

  The host uses it for the next call at once, and commits it once every message of this reading is accepted.
- `frames` — Text frames to send on the socket now, in order.
- `acknowledgments` — Protocol acknowledgments to send before acceptance, with no effect beyond confirming receipt.

  Answers to this reading's messages are sent only after these complete.
- `timer` — Milliseconds until a `timer` arrival, replacing any pending timer; absent keeps it.
  Timers end with the socket.
- `reconnect` — Close the socket and start the next read with `read`.

Implements:

```rust
impl Clone for Reading
impl Debug for Reading
impl PartialEq for Reading
```

## struct `bindings::exports::enco::plugin::channel::Request`

`src/lib.rs`

```rust
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: ::core::option::Option<Vec<u8>>,
    pub timeout_ms: ::core::option::Option<u32>,
}
```

A request for the host to send.

- `method` — The method, such as `GET` or `POST`.
- `url` — A path starting with `/`, joined to the connection's base URL, or an absolute URL at
  one of the mapper's `origins`.
- `headers` — Header names and values, in order.
- `body` — The bytes to send, if any.
- `timeout_ms` — The time limit in milliseconds; the host's default when absent.

Implements:

```rust
impl Clone for Request
impl Debug for Request
impl PartialEq for Request
```

## struct `bindings::exports::enco::plugin::channel::Response`

`src/lib.rs`

```rust
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
```

A response the protocol returned, whatever its status.

- `status` — The status code, such as 200.
- `headers` — Header names and values, in order.
- `body` — The whole body.

Implements:

```rust
impl Clone for Response
impl Debug for Response
impl PartialEq for Response
```

## struct `bindings::exports::enco::plugin::channel::Socket`

`src/lib.rs`

```rust
pub struct Socket {
    pub url: String,
    pub close_code: u16,
}
```

A WebSocket the host opens and keeps until the mapper asks to reconnect or the connection
stops.

- `url` — Where to open the socket: a path on the base URL, or an absolute `ws` or `wss` URL.

  A path starts with `/` and joins the base URL with `http` turned into `ws` and `https`
  into `wss`. An absolute URL must be on the base URL's host and port, or at one of the
  mapper's `origins`.
- `close_code` — The code the host closes the socket with; a protocol that resumes sessions across
  reconnects may need one other than 1000.

Implements:

```rust
impl Clone for Socket
impl Debug for Socket
impl PartialEq for Socket
```

## trait `bindings::exports::enco::plugin::channel::Guest`

`src/lib.rs`

```rust
pub trait Guest
```

### `bindings::exports::enco::plugin::channel::Guest::origins`

```rust
fn origins() -> Vec<String>
```

Additional origins for requests and sockets; `*.` before a host covers its subdomains. Read
when the generation loads.

### `bindings::exports::enco::plugin::channel::Guest::pacing`

```rust
fn pacing() -> ::core::option::Option<PacingGap>
```

The gap before each message `send` builds; absent sends each as soon as it is ready. Read
when the generation loads.

### `bindings::exports::enco::plugin::channel::Guest::identify`

```rust
fn identify() -> Request
```

The request that reads which account the connection reaches.

### `bindings::exports::enco::plugin::channel::Guest::identified`

```rust
fn identified(response: Response) -> ::core::result::Result<Identity, Fault>
```

The account in that request's response.

### `bindings::exports::enco::plugin::channel::Guest::read`

```rust
fn read(identity: Identity, state: ::core::option::Option<Json>) -> ::core::result::Result<ReadStep, Fault>
```

How to read next, from the latest protocol state; `state` is absent before the protocol
gave one.

`channel.invalid_state` means the state cannot be resumed. The host clears only that
state and retries from `none`, keeping routes and other committed business facts. If the
mapper rejects `none` too, the host stops the connection.

### `bindings::exports::enco::plugin::channel::Guest::received`

```rust
fn received(identity: Identity, state: ::core::option::Option<Json>, arrival: Arrival) -> ::core::result::Result<Reading, Fault>
```

What an arrival means, given the latest protocol state.

### `bindings::exports::enco::plugin::channel::Guest::fetch`

```rust
fn fetch(locator: String, responses: Vec<Response>) -> ::core::result::Result<FetchStep, Fault>
```

The next step of fetching the attachment at `locator`, given the responses so far.

Asking for more requests than the manual's limit for channel `fetch` is a contract
failure.

### `bindings::exports::enco::plugin::channel::Guest::split`

```rust
fn split(message: Outgoing) -> Vec<Outgoing>
```

The protocol messages to send, in order; at least one, none empty. Together they carry all
of the message's text and its file, laid out as the protocol needs.

### `bindings::exports::enco::plugin::channel::Guest::send`

```rust
fn send(conversation: String, part: Outgoing) -> ::core::result::Result<Request, Fault>
```

The request that sends one part to a conversation.

### `bindings::exports::enco::plugin::channel::Guest::answered`

```rust
fn answered(response: Response) -> ::core::result::Result<::core::option::Option<String>, Fault>
```

What the protocol answered: the ID of the message a request created, if any, or why it
failed.

### `bindings::exports::enco::plugin::channel::Guest::respond`

```rust
fn respond(handle: String, text: ::core::option::Option<String>) -> ::core::result::Result<::core::option::Option<Request>, Fault>
```

The request that answers the accepted message carrying `handle`, showing `text` to its
sender; absent when nothing more needs sending.

### `bindings::exports::enco::plugin::channel::Guest::commands`

```rust
fn commands(identity: Identity, commands: Vec<Command>) -> Vec<Request>
```

Requests that offer the host's commands in the protocol's menus, sent when a connection
starts.

### `bindings::exports::enco::plugin::channel::Guest::activity`

```rust
fn activity(conversation: String, shown: ::core::option::Option<String>, activity: ::core::option::Option<ActivityView>) -> ::core::option::Option<Request>
```

Creates or refreshes the activity view, or removes `shown` when `activity` is absent;
absent when nothing needs sending.

The host reads the response to a request made without `shown` with `answered`, and keeps
the message ID it gives as `shown`; without one, the Run shows no view.

### `bindings::exports::enco::plugin::channel::Guest::presence`

```rust
fn presence(conversation: String) -> ::core::option::Option<Request>
```

A passing sign in `conversation` that the Agent is working, such as typing; absent when
the protocol has none.

The host sends it when a Run starts and at each refresh until the Run ends, keeps nothing
of it, and reads its response with `answered`.

## type `bindings::exports::enco::plugin::channel::Failure`

`src/lib.rs`

```rust
pub type Failure = super::super::super::super::enco::plugin::types::Failure;
```

## type `bindings::exports::enco::plugin::channel::Json`

`src/lib.rs`

```rust
pub type Json = super::super::super::super::enco::plugin::types::Json;
```

## type `bindings::exports::enco::plugin::channel::Unviewable`

`src/lib.rs`

```rust
pub type Unviewable = super::super::super::super::enco::plugin::types::Unviewable;
```

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

- struct `bindings::exports::enco::plugin::lifecycle::Credential` — A secret the host writes into the plugin's requests or frames; the plugin never sees it.
- struct `bindings::exports::enco::plugin::lifecycle::Description` — Plugin-wide facts and requirements.
- struct `bindings::exports::enco::plugin::lifecycle::HeaderPlacement` — A request header that carries a credential.
- enum `bindings::exports::enco::plugin::lifecycle::Location` — Where the host writes a credential; a socket's opening handshake is a request like any other.
- struct `bindings::exports::enco::plugin::lifecycle::Placement` — One place a credential goes, and the only origin that receives it there.
- trait `bindings::exports::enco::plugin::lifecycle::Guest` — (no summary)
- type `bindings::exports::enco::plugin::lifecycle::Failure` — (no summary)
- type `bindings::exports::enco::plugin::lifecycle::Json` — (no summary)

## struct `bindings::exports::enco::plugin::lifecycle::Credential`

`src/lib.rs`

```rust
pub struct Credential {
    pub name: String,
    pub required: bool,
    pub placements: Vec<Placement>,
}
```

A secret the host writes into the plugin's requests or frames; the plugin never sees it.

- `name` — The name the owner maps to an environment variable.
- `required` — Whether registration fails without a secret mapping for it.
- `placements` — Every place the host writes the value, each bound to its origin; at least one.

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

## enum `bindings::exports::enco::plugin::lifecycle::Location`

`src/lib.rs`

```rust
pub enum Location {
    Header(HeaderPlacement),
    Path(Vec<String>),
    Frame(String),
}
```

Where the host writes a credential; a socket's opening handshake is a request like any
other.

- `Header` — A request header set to its prefix followed by the value.
- `Path` — Right after whichever of these prefixes the request path starts with.
- `Frame` — In each text frame sent on a socket, at this JSON pointer, where the frame holds `null`.

Implements:

```rust
impl Clone for Location
impl Debug for Location
impl PartialEq for Location
```

## struct `bindings::exports::enco::plugin::lifecycle::Placement`

`src/lib.rs`

```rust
pub struct Placement {
    pub origin: ::core::option::Option<String>,
    pub location: Location,
}
```

One place a credential goes, and the only origin that receives it there.

- `origin` — The destination origin; `*.` covers subdomains. Absent means the service the caller
  selects, such as a channel's base URL, by HTTP or WebSocket.
- `location` — Where in a request or frame to that origin.

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
