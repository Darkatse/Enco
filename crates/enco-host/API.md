<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_host`

`src/lib.rs`

Native storage, tools and context policies.

- module `channel` — A connection owns its routing state; adapters own only protocol interpretation.
- module `development` — Turns the plugin sources and `config.toml` in a home into registrations, and registered versions back into files.
- module `memory` — (no summary)
- module `status` — (no summary)
- module `telegram` — Telegram-specific configuration, transport evidence and text limits.
- struct `FactoryComposer` — Factory policy shared by ordinary operation and safe mode. No IO occurs here.
- struct `InstructionsContextSource` — Supplies the owner's current standing instructions once per Round.
- struct `Memories` — Sole entry point for memory writes and recall. The index never owns memory content.
- struct `MemoryContextSource` — (no summary)
- enum `MemoryError` — (no summary)
- struct `MemoryList` — (no summary)
- struct `MemoryPaths` — Authority and derived-index locations, both owned by Memories.
- struct `Recall` — Current authority records nominated by the index, plus recent unindexed records.
- struct `SqliteStore` — SQLite authority and its associated content and identity files.
- struct `StorePaths` — Files owned by the persistence adapter.
- struct `SystemClock` — Host wall clock; Log order is assigned independently by the Session.
- struct `Telegram` — (no summary)
- fn `blob_path` — Address a blob without opening the store.
- fn `development_tool_specs` — Declarations are available before the registry binds their handlers.
- fn `development_tools` — File operations and registration follow the same path used by the CLI.
- fn `memory_tool_specs` — Native declarations available before their stateful handlers are constructed.
- fn `memory_tools` — (no summary)
- fn `native_tools` — Native capabilities share the workspace path rules and ordinary Tool dispatch.
- fn `self_status_spec` — The declaration of `self_status`, whose handler belongs to the application.
- const `LIFELINE` — Minimal repair tools, also required by ordinary Agent Sessions.
- const `TABLE_OWNERS` — Configuration ownership for the host components assembled by the application.

## struct `FactoryComposer`

`src/composer.rs`

```rust
pub struct FactoryComposer {
    /* private fields */
}
```

Factory policy shared by ordinary operation and safe mode. No IO occurs here.

### `FactoryComposer::new`

```rust
pub fn new(home: PathBuf, workspace: PathBuf, instructions: PathBuf) -> Self
```

Implements:

```rust
impl Composer for FactoryComposer
```

## struct `InstructionsContextSource`

`src/context.rs`

```rust
pub struct InstructionsContextSource {
    /* private fields */
}
```

Supplies the owner's current standing instructions once per Round.

### `InstructionsContextSource::new`

```rust
pub fn new(path: PathBuf) -> Self
```

Implements:

```rust
impl ContextSource for InstructionsContextSource
```

## struct `Memories`

`src/memory.rs`

```rust
pub struct Memories {
    /* private fields */
}
```

Sole entry point for memory writes and recall. The index never owns memory content.

### `Memories::open`

```rust
pub async fn open(paths: MemoryPaths, registry: Arc<Registry>, clock: Arc<dyn Clock>) -> Result<Arc<Self>, MemoryError>
```

### `Memories::save`

```rust
pub async fn save(&self, text: String, pinned: bool) -> Result<Memory, MemoryError>
```

### `Memories::update`

```rust
pub async fn update(&self, id: MemoryId, text: Option<String>, pinned: Option<bool>) -> Result<Option<Memory>, MemoryError>
```

### `Memories::forget`

```rust
pub async fn forget(&self, id: MemoryId) -> Result<bool, MemoryError>
```

### `Memories::pinned`

```rust
pub async fn pinned(&self) -> Result<Vec<Memory>, MemoryError>
```

### `Memories::list`

```rust
pub async fn list(&self) -> Result<MemoryList, MemoryError>
```

Inspect the authority and index revisions without contacting the embedding service.

### `Memories::recall`

```rust
pub async fn recall(&self, query: &str, limit: usize, cancel: &CancellationToken) -> Result<Recall, MemoryError>
```

Synchronize and search under one index lock, then resolve IDs against the authority.

## struct `MemoryContextSource`

`src/memory/context.rs`

```rust
pub struct MemoryContextSource {
    /* private fields */
}
```

### `MemoryContextSource::new`

```rust
pub fn new(memories: Arc<Memories>) -> Self
```

Implements:

```rust
impl ContextSource for MemoryContextSource
```

## enum `MemoryError`

`src/memory.rs`

```rust
pub enum MemoryError {
    Registry(enco_kernel::RegistryError),
    Authority(String),
    SchemaVersion {
        found: u32,
        expected: u32,
    },
    Index(String),
    Cancelled,
}
```

Implements:

```rust
impl Debug for MemoryError
impl Display for MemoryError
impl Error for MemoryError
impl From<RegistryError> for MemoryError
```

## struct `MemoryList`

`src/memory.rs`

```rust
pub struct MemoryList {
    pub memories: Vec<Memory>,
    pub unindexed: Vec<MemoryId>,
}
```

Implements:

```rust
impl<'de> Deserialize<'de> for MemoryList
impl Serialize for MemoryList
```

## struct `MemoryPaths`

`src/memory.rs`

```rust
pub struct MemoryPaths {
    pub db: std::path::PathBuf,
    pub index: std::path::PathBuf,
}
```

Authority and derived-index locations, both owned by Memories.

## struct `Recall`

`src/memory.rs`

```rust
pub struct Recall {
    pub memories: Vec<Memory>,
    pub unindexed: Vec<Memory>,
    pub lexical_only: Option<Failure>,
}
```

Current authority records nominated by the index, plus recent unindexed records.

- `lexical_only` — Present when embedding failed; callers must disclose keyword-only recall.

Implements:

```rust
impl<'de> Deserialize<'de> for Recall
impl Serialize for Recall
```

## struct `SqliteStore`

`src/store.rs`

```rust
pub struct SqliteStore {
    /* private fields */
}
```

SQLite authority and its associated content and identity files.

### `SqliteStore::open`

```rust
pub async fn open(paths: StorePaths) -> Result<Self, StoreError>
```

Open the authority, initializing the schema and node identity on first use.

Implements:

```rust
impl Store for SqliteStore
```

## struct `StorePaths`

`src/store.rs`

```rust
pub struct StorePaths {
    pub db: std::path::PathBuf,
    pub blobs: std::path::PathBuf,
    pub artifacts: std::path::PathBuf,
}
```

Files owned by the persistence adapter.

- `db` — SQLite authority.
- `blobs` — Removable content-addressed history.
- `artifacts` — Retained content-addressed components.

## struct `SystemClock`

`src/clock.rs`

```rust
pub struct SystemClock;
```

Host wall clock; Log order is assigned independently by the Session.

Implements:

```rust
impl Clock for SystemClock
```

## struct `Telegram`

`src/telegram.rs`

```rust
pub struct Telegram {
    /* private fields */
}
```

### `Telegram::new`

```rust
pub fn new(token: String, api_base: &str) -> Result<Self, ChannelError>
```

Implements:

```rust
impl Adapter for Telegram
```

## fn `blob_path`

`src/store/blob.rs`

```rust
pub fn blob_path(blobs: &std::path::Path, hash: &enco_core::ContentHash) -> std::path::PathBuf
```

Address a blob without opening the store.

## fn `development_tool_specs`

`src/tools/development.rs`

```rust
pub fn development_tool_specs() -> Result<Vec<ToolSpec>, SchemaError>
```

Declarations are available before the registry binds their handlers.

## fn `development_tools`

`src/tools/development.rs`

```rust
pub fn development_tools(development: std::sync::Arc<crate::development::Development>, registry: std::sync::Arc<enco_kernel::Registry>) -> Result<Vec<std::sync::Arc<dyn Tool>>, SchemaError>
```

File operations and registration follow the same path used by the CLI.

## fn `memory_tool_specs`

`src/memory/tools.rs`

```rust
pub fn memory_tool_specs() -> Result<Vec<ToolSpec>, SchemaError>
```

Native declarations available before their stateful handlers are constructed.

## fn `memory_tools`

`src/memory/tools.rs`

```rust
pub fn memory_tools(memories: std::sync::Arc<super::Memories>) -> Result<Vec<std::sync::Arc<dyn Tool>>, SchemaError>
```

## fn `native_tools`

`src/tools.rs`

```rust
pub fn native_tools(workspace: std::path::PathBuf) -> Result<Vec<std::sync::Arc<dyn Tool>>, enco_core::SchemaError>
```

Native capabilities share the workspace path rules and ordinary Tool dispatch.

## fn `self_status_spec`

`src/development/call.rs`

```rust
pub fn self_status_spec() -> enco_core::ToolSpec
```

The declaration of `self_status`, whose handler belongs to the application.

## const `LIFELINE`

`src/tools.rs`

```rust
pub const LIFELINE: [&str; 8] = _;
```

Minimal repair tools, also required by ordinary Agent Sessions.

## const `TABLE_OWNERS`

`src/lib.rs`

```rust
pub const TABLE_OWNERS: [enco_kernel::TableOwner; 2] = _;
```

Configuration ownership for the host components assembled by the application.

# module `enco_host::channel`

`src/channel.rs`

A connection owns its routing state; adapters own only protocol interpretation.

- enum `channel::Action` — Meaning interpreted by the host, independent of protocol command syntax.
- struct `channel::Attachment` — (no summary)
- enum `channel::AttachmentContent` — (no summary)
- struct `channel::Channel` — Lifetime handle; shutdown signals and joins the connection actor and all its helpers.
- enum `channel::ChannelError` — (no summary)
- struct `channel::ChannelStatus` — Observable connection health and durable final delivery failures.
- struct `channel::Fault` — Protocol evidence used by the common retry and settlement path.
- struct `channel::Identity` — Stable identity declared by a protocol adapter.
- struct `channel::Input` — (no summary)
- struct `channel::MessageContent` — (no summary)
- struct `channel::Update` — One protocol update with its resulting protocol state.
- trait `channel::Adapter` — Native adapter boundary, replaced by the channel plugin boundary in P3. Outbound Markdown and optional files are mapped to the channel protocol here.

## enum `channel::Action`

`src/channel.rs`

```rust
pub enum Action<T = Attachment> {
    Message(MessageContent<T>),
    Sessions,
    SelectSession(String),
    Cancel,
}
```

Meaning interpreted by the host, independent of protocol command syntax.

## struct `channel::Attachment`

`src/channel.rs`

```rust
pub struct Attachment {
    pub what: String,
    pub locator: String,
}
```

## enum `channel::AttachmentContent`

`src/channel.rs`

```rust
pub enum AttachmentContent {
    Image(Vec<u8>),
    Unavailable(String),
}
```

Implements:

```rust
impl Debug for AttachmentContent
```

## struct `channel::Channel`

`src/channel.rs`

```rust
pub struct Channel {
    /* private fields */
}
```

Lifetime handle; shutdown signals and joins the connection actor and all its helpers.

### `channel::Channel::start`

```rust
pub fn start(kernel: Arc<Kernel>, adapter: Arc<dyn Adapter>, owner_id: String, clock: Arc<dyn Clock>) -> Self
```

### `channel::Channel::status`

```rust
pub async fn status(&self) -> Result<ChannelStatus, KernelError>
```

### `channel::Channel::shutdown`

```rust
pub async fn shutdown(&self) -> Result<(), ChannelError>
```

Implements:

```rust
impl Drop for Channel
```

## enum `channel::ChannelError`

`src/channel.rs`

```rust
pub enum ChannelError {
    Kernel(enco_kernel::KernelError),
    State(serde_json::Error),
    Protocol(Fault),
    Task(String),
    Unjoined(tokio::task::JoinError),
    Config(String),
}
```

- `Unjoined` — The owner did not complete cleanup, so its helpers may still be running.

Implements:

```rust
impl Debug for ChannelError
impl Display for ChannelError
impl Error for ChannelError
impl From<Error> for ChannelError
impl From<Fault> for ChannelError
impl From<KernelError> for ChannelError
```

## struct `channel::ChannelStatus`

`src/channel.rs`

```rust
pub struct ChannelStatus {
    pub key: String,
    pub running: bool,
    pub stopped: Option<String>,
    pub recent_failures: Vec<enco_kernel::DeliveryStatus>,
}
```

Observable connection health and durable final delivery failures.

Implements:

```rust
impl Serialize for ChannelStatus
```

## struct `channel::Fault`

`src/channel.rs`

```rust
pub struct Fault {
    pub failure: Failure,
    pub unknown: bool,
    pub fatal: bool,
    pub retry_after: Option<std::time::Duration>,
}
```

Protocol evidence used by the common retry and settlement path.

- `unknown` — True when effects may have occurred; this always forbids an automatic resend.
- `fatal` — True when the whole connection is unusable, rather than just this destination.

Implements:

```rust
impl Debug for Fault
impl Display for Fault
impl Error for Fault
impl From<Fault> for ChannelError
```

## struct `channel::Identity`

`src/channel.rs`

```rust
pub struct Identity {
    pub channel: String,
    pub account: String,
}
```

Stable identity declared by a protocol adapter.

### `channel::Identity::key`

```rust
pub fn key(&self) -> String
```

Implements:

```rust
impl Clone for Identity
```

## struct `channel::Input`

`src/channel.rs`

```rust
pub struct Input {
    pub conversation: String,
    pub sender: Option<String>,
    pub conversation_type: String,
    pub direct: bool,
    pub action: Option<Action<Attachment>>,
}
```

- `conversation_type` — Adapter-provided label retained for diagnostics, not interpreted by the connection.

## struct `channel::MessageContent`

`src/channel.rs`

```rust
pub struct MessageContent<T> {
    pub text: String,
    pub images: Vec<T>,
    pub unviewable: Vec<Unviewable>,
}
```

Implements:

```rust
impl<T> From<&str> for MessageContent<T>
impl<T> From<String> for MessageContent<T>
```

## struct `channel::Update`

`src/channel.rs`

```rust
pub struct Update {
    pub protocol: serde_json::Value,
    pub input: Option<Input>,
}
```

One protocol update with its resulting protocol state.

## trait `channel::Adapter`

`src/channel.rs`

```rust
pub trait Adapter: Send + Sync
```

Native adapter boundary, replaced by the channel plugin boundary in P3.
Outbound Markdown and optional files are mapped to the channel protocol here.

### `channel::Adapter::identity`

```rust
fn identity(&self) -> Identity
```

Declare the connection identity; it must remain stable for this adapter instance.

### `channel::Adapter::initial_state`

```rust
fn initial_state(&self) -> Value
```

Protocol state used only when no connection state has been accepted yet.

### `channel::Adapter::poll`

```rust
async fn poll(&self, protocol: Value) -> Result<Vec<Update>, Fault>
```

Read replayable updates using committed state; cancellation has no external effect.
Each returned update carries the protocol state after accepting that update.

### `channel::Adapter::fetch`

```rust
async fn fetch(&self, locator: &str) -> Result<AttachmentContent, Fault>
```

Fetch a protocol-owned attachment after authorization; reads have no external effect.

### `channel::Adapter::split`

```rust
fn split(&self, message: Outgoing<FileData>) -> Vec<Outgoing<FileData>>
```

Preserve all text; attach the file to the last part, or a separate final part.
Each part has text or a file; a file-only message produces exactly one part.

### `channel::Adapter::send`

```rust
async fn send(&self, target: &str, message: &Outgoing<FileData>) -> Result<(), Fault>
```

Perform one bounded attempt. Never retry internally or drop an in-flight side effect.

# module `enco_host::development`

`src/development.rs`

Turns the plugin sources and `config.toml` in a home into registrations, and registered versions
back into files.

The owner and the Agent edit those files; what is registered and what runs live in Enco's store,
and only the registry changes them.

- module `development::tree` — A directory as an exact value: file bytes by canonical relative path.
- struct `development::AgentReason` — A suggested call's reason, which the Agent writes.
- struct `development::Applied` — The result of an apply: what was registered, and what each build did.
- struct `development::ApplyArguments` — (no summary)
- enum `development::BuildFailure` — (no summary)
- struct `development::BuildResult` — What happened to one plugin source during an apply.
- struct `development::Comparison` — (no summary)
- enum `development::ComparisonMode` — (no summary)
- struct `development::ConfigurationUnit` — (no summary)
- enum `development::ContentFailure` — Why content cannot be read; other failures stop the comparison.
- struct `development::ContentUnavailable` — (no summary)
- struct `development::Development` — Entry point for the plugin development tools in one home.
- enum `development::DevelopmentCall` — One parsed call of a development tool. Suggestions use `AgentReason` where the Agent writes.
- enum `development::DevelopmentError` — (no summary)
- struct `development::DevelopmentStatus` — The three layers of every plugin and of `config.toml`, observed once.
- enum `development::DevelopmentTool` — (no summary)
- struct `development::EntryError` — (no summary)
- enum `development::EntryProblem` — (no summary)
- struct `development::Failed` — (no summary)
- struct `development::FileDiff` — (no summary)
- enum `development::FileDifference` — (no summary)
- struct `development::Following` — Keeps a manual in step with the registry until shutdown.
- struct `development::History` — (no summary)
- enum `development::HistoryPoint` — (no summary)
- enum `development::HistoryQuery` — (no summary)
- enum `development::HomeComparison` — Byte equality and meaning are distinct facts; reading a comparison never builds source.
- struct `development::HomePlugin` — (no summary)
- struct `development::Invocation` — A suggested call, serialized only where a document or JSON result is written.
- struct `development::Manual` — Writes the manual of one home.
- enum `development::Operation` — The development operation that failed; it decides what a failure left behind and how to repeat the operation.
- struct `development::PluginUnit` — (no summary)
- struct `development::RegisteredUnit` — (no summary)
- struct `development::RenameArguments` — (no summary)
- struct `development::RewindArguments` — (no summary)
- struct `development::RewindConflict` — (no summary)
- struct `development::Rewound` — (no summary)
- struct `development::ScaffoldArguments` — (no summary)
- struct `development::Sdk` — The plugin SDK sources embedded in the host binary, written to `sdk/` in each home.
- enum `development::SettingsComparison` — (no summary)
- struct `development::StatusArguments` — (no summary)
- enum `development::SuggestedArguments` — (no summary)
- enum `development::ToolchainProblem` — Why the SDK's toolchain cannot build plugins on this machine.
- struct `development::UnitDiff` — (no summary)
- fn `development::registration_document` — (no summary)
- fn `development::self_status_spec` — same as `self_status_spec`
- const `development::APPLY` — (no summary)
- const `development::HISTORY` — (no summary)
- const `development::HISTORY_PAGE_SIZE` — (no summary)
- const `development::MANUAL_DIRECTORY` — The directory under the home that holds the manual.
- const `development::RENAME` — (no summary)
- const `development::REWIND` — (no summary)
- const `development::SCAFFOLD` — (no summary)
- const `development::STATUS` — (no summary)

## struct `development::AgentReason`

`src/development/arguments.rs`

```rust
pub struct AgentReason;
```

A suggested call's reason, which the Agent writes.

It serializes as the placeholder that reason parsing rejects, so a suggestion copied unchanged
cannot register.

Implements:

```rust
impl Clone for AgentReason
impl Copy for AgentReason
impl Debug for AgentReason
impl Eq for AgentReason
impl PartialEq for AgentReason
impl Serialize for super::AgentReason
```

## struct `development::Applied`

`src/development.rs`

```rust
pub struct Applied {
    pub registered: Option<enco_kernel::Registered>,
    pub builds: Vec<BuildResult>,
    pub text_only: Vec<enco_core::IntentPath>,
}
```

The result of an apply: what was registered, and what each build did.

- `registered` — The registration; absent when nothing differs from what is registered and no factory plugin
  has newer host code to follow.
- `builds` — One result for each plugin source that differs from what is registered or was built with an
  earlier SDK; unchanged sources are skipped.
- `text_only` — Plugin directories or `config.toml` whose bytes changed but whose meaning did not, as when
  only a comment was edited; they stay unregistered.

### `development::Applied::document`

`src/development/render/actions.rs`

```rust
pub fn document(&self) -> String
```

Implements:

```rust
impl Serialize for Applied
```

## struct `development::ApplyArguments`

`src/development/arguments.rs`

```rust
pub struct ApplyArguments<R = enco_core::Reason> {
    pub reason: R,
}
```

- `reason` — Why this change is registered, in one sentence; history shows it.

Implements:

```rust
impl<R: Clone> Clone for ApplyArguments<R>
impl<R: Debug> Debug for ApplyArguments<R>
impl<'de, R> Deserialize<'de> for ApplyArguments<R> where R: Deserialize<'de>
impl<R: Eq> Eq for ApplyArguments<R>
impl<R> JsonSchema for ApplyArguments<R> where R: schemars::JsonSchema
impl<R: PartialEq> PartialEq for ApplyArguments<R>
impl<R> Serialize for ApplyArguments<R> where R: Serialize
```

## enum `development::BuildFailure`

`src/development/error.rs`

```rust
pub enum BuildFailure {
    Cargo {
        stage: &'static str,
        diagnostics: Vec<String>,
        log: std::path::PathBuf,
    },
    NoArtifact(std::path::PathBuf),
    EmptySettings(enco_kernel::LoadError),
    SourceChanged,
    Toolchain {
        channel: String,
        problem: ToolchainProblem,
    },
}
```

- `Cargo`
  - `diagnostics` — Diagnostic excerpts, or the error output when Cargo reported none.
- `Toolchain` — Checked before Cargo runs; installing a toolchain downloads it, so the owner repairs this.
  - `channel` — The toolchain the SDK pins, as rustup names it.

Implements:

```rust
impl Debug for BuildFailure
impl Display for BuildFailure
impl Error for BuildFailure
```

## struct `development::BuildResult`

`src/development/build.rs`

```rust
pub struct BuildResult {
    pub name: enco_core::PluginName,
    pub reused: bool,
    pub code_changed: bool,
    pub diagnostics: Vec<String>,
    pub log: Option<std::path::PathBuf>,
}
```

What happened to one plugin source during an apply.

- `name` — Plugin whose source was built or reused.
- `reused` — Whether a stored build of the same source and SDK was used, so Cargo did not run.
- `code_changed` — Whether the component differs from the registered one, or none is registered; only then is
  the new code registered.
- `diagnostics` — At most `BUILD_DIAGNOSTICS` distinct compiler messages, errors first; empty when the build
  was reused.
- `log` — Path to Cargo's complete output; absent when the build was reused.

Implements:

```rust
impl Serialize for BuildResult
```

## struct `development::Comparison`

`src/development/history.rs`

```rust
pub struct Comparison {
    pub from: Option<HistoryPoint>,
    pub to: HistoryPoint,
    pub mode: ComparisonMode,
    pub units: Vec<UnitDiff>,
    pub conflicting_paths: Vec<IntentPath>,
    pub next: Option<crate::status::Steps>,
}
```

- `from` — Absent only before the first registration.
- `conflicting_paths` — Different identities would require the same current directory.
- `next` — Actions include only paths addressable by the rewind tool.

Implements:

```rust
impl Serialize for Comparison
```

## enum `development::ComparisonMode`

`src/development/history.rs`

```rust
pub enum ComparisonMode {
    HomeChanges,
    RewindPreview,
    Registrations,
}
```

Implements:

```rust
impl Eq for ComparisonMode
impl PartialEq for ComparisonMode
impl Serialize for ComparisonMode
```

## struct `development::ConfigurationUnit`

`src/development/status.rs`

```rust
pub struct ConfigurationUnit {
    pub running: enco_core::ConfigurationRef,
    pub registered: enco_core::ConfigurationRef,
    pub home: super::HomeComparison,
    pub invalid: Vec<enco_kernel::SettingsError>,
    pub unavailable_uses: Vec<enco_kernel::UnservedUse>,
}
```

Implements:

```rust
impl Serialize for ConfigurationUnit
```

## enum `development::ContentFailure`

`src/development/history/content.rs`

```rust
pub enum ContentFailure {
    Missing {
        hash: ContentHash,
    },
    Corrupt {
        reason: String,
    },
    Unreadable(crate::development::EntryError),
}
```

Why content cannot be read; other failures stop the comparison.

Implements:

```rust
impl Debug for ContentFailure
impl Serialize for ContentFailure
```

## struct `development::ContentUnavailable`

`src/development/history/content.rs`

```rust
pub struct ContentUnavailable {
    pub point: super::HistoryPoint,
    pub cause: ContentFailure,
}
```

Implements:

```rust
impl Debug for ContentUnavailable
impl Serialize for ContentUnavailable
```

## struct `development::Development`

`src/development.rs`

```rust
pub struct Development {
    /* private fields */
}
```

Entry point for the plugin development tools in one home.

Operations that write files or register run one at a time; status and history do not wait for
them.

### `development::Development::history`

`src/development/history.rs`

```rust
pub async fn history(&self, registry: &Registry, query: HistoryQuery) -> Result<History, Failed>
```

Compare file values by plugin identity; names only locate their current paths.

### `development::Development::open`

`src/development/registration.rs`

```rust
pub async fn open(&self, deps: RegistryDeps) -> Result<Arc<Registry>, Failed>
```

Restore accepted behavior, then offer host changes through ordinary registration.

### `development::Development::apply`

`src/development/registration.rs`

```rust
pub async fn apply(&self, registry: &Registry, initiator: Initiator, cancel: &CancellationToken) -> Result<Applied, Failed>
```

Capture source and configuration once, then offer the complete change to the registry.

### `development::Development::deploy`

`src/development/registration.rs`

```rust
pub async fn deploy(&self, registry: &Registry, name: PluginName, bytes: Vec<u8>, initiator: Initiator, cancel: &CancellationToken) -> Result<Registered, Failed>
```

Direct artifacts share acceptance and history while leaving file intent unchanged.

### `development::Development::rename`

`src/development/rename.rs`

```rust
pub async fn rename(&self, registry: &Registry, name: &PluginName, to: &PluginName, initiator: Initiator, cancel: &CancellationToken) -> Result<Registered, Failed>
```

### `development::Development::rewind`

`src/development/rewind.rs`

```rust
pub async fn rewind(&self, registry: &Registry, arguments: RewindArguments, cancel: &CancellationToken) -> Result<Rewound, Failed>
```

### `development::Development::scaffold`

`src/development/scaffold.rs`

```rust
pub async fn scaffold(&self, registry: &Registry, name: &PluginName) -> Result<Vec<PathBuf>, Failed>
```

Write a standalone tool plugin without registering an identity or changing running code.

### `development::Development::status`

`src/development/status.rs`

```rust
pub async fn status(&self, registry: &Registry) -> Result<DevelopmentStatus, Failed>
```

Inspect source and registry facts without building or changing either of them.

### `development::Development::new`

```rust
pub fn new(home: PathBuf, sdk: Sdk, manual: Manual, store: Arc<SqliteStore>, runtime: Arc<dyn Runtime>) -> Arc<Self>
```

Bind development to `home`; nothing is read or written until an operation runs.

### `development::Development::manual`

```rust
pub fn manual(&self) -> &Arc<Manual>
```

The manual of this home, which every registration made here rewrites before it returns.

### `development::Development::initialize`

```rust
pub async fn initialize(home: &Path) -> Result<(), EntryError>
```

Create `plugins/` in a new home; an existing directory is left as it is.

## enum `development::DevelopmentCall`

`src/development/call.rs`

```rust
pub enum DevelopmentCall<R = enco_core::Reason> {
    Apply(super::ApplyArguments<R>),
    History(super::HistoryQuery),
    Rewind(super::RewindArguments),
    Rename(super::RenameArguments<R>),
    Scaffold(super::ScaffoldArguments),
}
```

One parsed call of a development tool. Suggestions use `AgentReason` where the Agent writes.

### `development::DevelopmentCall::tool`

```rust
pub fn tool(&self) -> DevelopmentTool
```

Implements:

```rust
impl<R: Clone> Clone for DevelopmentCall<R>
impl<R: Debug> Debug for DevelopmentCall<R>
impl<R: Eq> Eq for DevelopmentCall<R>
impl<R: PartialEq> PartialEq for DevelopmentCall<R>
impl<R> Serialize for DevelopmentCall<R> where R: Serialize
```

## enum `development::DevelopmentError`

`src/development/error.rs`

```rust
pub enum DevelopmentError {
    Entry(EntryError),
    Build {
        name: enco_core::PluginName,
        failure: BuildFailure,
    },
    Registry(enco_kernel::RegistryError),
    Store(enco_kernel::StoreError),
    Corrupt(String),
    Host(String),
    NameTaken {
        name: enco_core::PluginName,
    },
    DirectoryExists {
        name: enco_core::PluginName,
    },
    NoSource {
        name: enco_core::PluginName,
    },
    AmbiguousRename {
        missing: Vec<enco_core::PluginName>,
        added: Vec<enco_core::PluginName>,
        latest: enco_core::RegistrationId,
    },
    Preparation(Vec<DevelopmentError>),
    Moved(Box<DevelopmentError>),
    Partial {
        written: Vec<std::path::PathBuf>,
        source: Box<DevelopmentError>,
    },
    RewindRefused {
        conflicts: Vec<super::RewindConflict>,
        selected: Vec<enco_core::IntentPath>,
    },
    RewindPath {
        path: enco_core::IntentPath,
    },
    RewindCollision {
        path: enco_core::IntentPath,
    },
    RewindContent {
        path: enco_core::IntentPath,
        unavailable: super::ContentUnavailable,
    },
    WriteConflicts {
        path: std::path::PathBuf,
        conflicts: Vec<super::tree::WriteConflict>,
    },
    Cancelled,
}
```

- `AmbiguousRename` — Registered sources disappeared while new directories appeared; a rename is ambiguous.
- `Preparation` — Some sources or `config.toml` could not be prepared; each failure names its subject.
- `Moved` — The rename failed after its directory moved.
- `Partial` — The operation stopped after these file operations completed.
- `RewindRefused`
  - `selected` — Every path the rewind selected.

### `development::DevelopmentError::code`

```rust
pub fn code(&self) -> &'static str
```

The stable classification of the failure, decided where each variant is added.

Implements:

```rust
impl Debug for DevelopmentError
impl Display for DevelopmentError
impl Error for DevelopmentError
impl From<EntryError> for DevelopmentError
impl From<InvalidSourcePath> for DevelopmentError
impl From<RegistryError> for DevelopmentError
impl From<StoreError> for DevelopmentError
```

## struct `development::DevelopmentStatus`

`src/development/status.rs`

```rust
pub struct DevelopmentStatus {
    pub registration: enco_core::RegistrationId,
    pub plugins: std::collections::BTreeMap<enco_core::PluginName, PluginUnit>,
    pub configuration: ConfigurationUnit,
    pub recent_registrations: Vec<enco_kernel::RegistrationDetails>,
    pub plugins_unreadable: Option<super::EntryError>,
}
```

The three layers of every plugin and of `config.toml`, observed once.

- `plugins` — Every plugin that runs, is registered, or has a directory.
- `plugins_unreadable` — Why the `plugins` directory cannot be listed; the home facts of plugins are then absent.

Implements:

```rust
impl Serialize for DevelopmentStatus
```

## enum `development::DevelopmentTool`

`src/development/call.rs`

```rust
pub enum DevelopmentTool {
    Apply,
    History,
    Rewind,
    Rename,
    Scaffold,
}
```

### `development::DevelopmentTool::ALL`

```rust
const ALL: [Self; 5] = _;
```

### `development::DevelopmentTool::name`

```rust
pub const fn name(self) -> &'static str
```

### `development::DevelopmentTool::named`

```rust
pub fn named(name: &str) -> Option<Self>
```

### `development::DevelopmentTool::parse`

```rust
pub fn parse(self, arguments: Map<String, Value>) -> Result<DevelopmentCall, Failure>
```

The parser execution uses, and the only one.

### `development::DevelopmentTool::spec`

```rust
pub fn spec(self) -> Result<ToolSpec, SchemaError>
```

Implements:

```rust
impl Clone for DevelopmentTool
impl Copy for DevelopmentTool
impl Debug for DevelopmentTool
impl Eq for DevelopmentTool
impl PartialEq for DevelopmentTool
```

## struct `development::EntryError`

`src/development/error.rs`

```rust
pub struct EntryError {
    pub path: std::path::PathBuf,
    pub problem: EntryProblem,
}
```

Implements:

```rust
impl Clone for EntryError
impl Debug for EntryError
impl Display for EntryError
impl Eq for EntryError
impl Error for EntryError
impl From<EntryError> for DevelopmentError
impl PartialEq for EntryError
impl Serialize for EntryError
```

## enum `development::EntryProblem`

`src/development/error.rs`

```rust
pub enum EntryProblem {
    Io(String),
    NotDirectory,
    NotRegular,
    NotUtf8,
    UnsupportedName,
    NotText,
    IgnoreRule(String),
    NotPluginName,
    Toml(String),
    ReadmeMarkers,
}
```

- `UnsupportedName` — Source paths use `/` as their only separator, so names cannot contain `\\`.

Implements:

```rust
impl Clone for EntryProblem
impl Debug for EntryProblem
impl Display for EntryProblem
impl Eq for EntryProblem
impl Error for EntryProblem
impl PartialEq for EntryProblem
impl Serialize for EntryProblem
```

## struct `development::Failed`

`src/development/error.rs`

```rust
pub struct Failed {
    pub operation: Operation,
    pub error: Box<DevelopmentError>,
}
```

### `development::Failed::next`

`src/development/error/next.rs`

```rust
pub fn next(&self) -> Steps
```

### `development::Failed::code`

```rust
pub fn code(&self) -> &'static str
```

Implements:

```rust
impl Debug for Failed
impl Display for Failed
impl Error for Failed
```

## struct `development::FileDiff`

`src/development/history.rs`

```rust
pub struct FileDiff {
    pub path: String,
    pub change: FileDifference,
}
```

Implements:

```rust
impl Serialize for FileDiff
```

## enum `development::FileDifference`

`src/development/history.rs`

```rust
pub enum FileDifference {
    Text {
        diff: String,
    },
    AddedEmpty,
    RemovedEmpty,
    Binary,
}
```

Implements:

```rust
impl Serialize for FileDifference
```

## struct `development::Following`

`src/development/manual.rs`

```rust
pub struct Following {
    /* private fields */
}
```

Keeps a manual in step with the registry until shutdown.

### `development::Following::shutdown`

```rust
pub async fn shutdown(&self) -> Result<(), tokio::task::JoinError>
```

Stop following, after any write in progress finishes.

Implements:

```rust
impl Drop for Following
```

## struct `development::History`

`src/development/history.rs`

```rust
pub struct History {
    pub registrations: Vec<enco_kernel::RegistrationDetails>,
    pub before: Option<RegistrationId>,
    pub comparison: Option<Comparison>,
    pub timezone: Tz,
}
```

- `before` — The cursor for older registrations, when more exist.

### `development::History::document`

`src/development/render/history.rs`

```rust
pub fn document(&self) -> String
```

Implements:

```rust
impl Serialize for History
```

## enum `development::HistoryPoint`

`src/development/history.rs`

```rust
pub enum HistoryPoint {
    Registration(RegistrationId),
    Home,
}
```

Implements:

```rust
impl Clone for HistoryPoint
impl Copy for HistoryPoint
impl Debug for HistoryPoint
impl<'de> Deserialize<'de> for HistoryPoint
impl Eq for HistoryPoint
impl FromStr for HistoryPoint
impl JsonSchema for HistoryPoint
impl PartialEq for HistoryPoint
impl Serialize for HistoryPoint
```

## enum `development::HistoryQuery`

`src/development/history.rs`

```rust
pub enum HistoryQuery {
    List {
        before: Option<RegistrationId>,
    },
    Compare {
        from: Option<HistoryPoint>,
        to: HistoryPoint,
    },
}
```

- `Compare` — Two intent values. Without `from`, the home is compared with the latest
  registration, and a registration with the one before it.

### `development::HistoryQuery::new`

```rust
pub fn new(from: Option<HistoryPoint>, to: Option<HistoryPoint>, before: Option<RegistrationId>) -> Result<Self, &'static str>
```

The one rule for the tool's and the CLI's arguments: a cursor lists, and a side compares.

Implements:

```rust
impl Clone for HistoryQuery
impl Debug for HistoryQuery
impl<'de> Deserialize<'de> for HistoryQuery
impl Eq for HistoryQuery
impl JsonSchema for HistoryQuery
impl PartialEq for HistoryQuery
impl Serialize for HistoryQuery
```

## enum `development::HomeComparison`

`src/development/home.rs`

```rust
pub enum HomeComparison {
    Same,
    TextOnly,
    Changed,
    Unbuilt,
    Removed,
    Absent,
    Unreadable(super::EntryError),
}
```

Byte equality and meaning are distinct facts; reading a comparison never builds source.

- `Unbuilt` — The bytes differ, and no build tells yet whether the code does.
- `Absent` — There is no directory and no registered source.

Implements:

```rust
impl Clone for HomeComparison
impl Debug for HomeComparison
impl Eq for HomeComparison
impl PartialEq for HomeComparison
impl Serialize for HomeComparison
```

## struct `development::HomePlugin`

`src/development/home.rs`

```rust
pub struct HomePlugin {
    pub code: HomeComparison,
    pub settings: SettingsComparison,
}
```

Implements:

```rust
impl Serialize for HomePlugin
```

## struct `development::Invocation`

`src/development/call.rs`

```rust
pub struct Invocation {
    pub tool: &'static str,
    pub arguments: SuggestedArguments,
    pub agent_supplies: Vec<&'static str>,
}
```

A suggested call, serialized only where a document or JSON result is written.

### `development::Invocation::new`

```rust
pub fn new(call: DevelopmentCall<AgentReason>) -> Self
```

### `development::Invocation::status`

```rust
pub fn status() -> Self
```

Implements:

```rust
impl Clone for Invocation
impl Debug for Invocation
impl Serialize for Invocation
```

## struct `development::Manual`

`src/development/manual.rs`

```rust
pub struct Manual {
    /* private fields */
}
```

Writes the manual of one home.

### `development::Manual::new`

```rust
pub fn new(home: &Path, host: &[(&str, &[u8])], limits: Vec<(String, String)>, store: Arc<SqliteStore>) -> Result<Self, DevelopmentError>
```

Bind the manual of `home` to the documents the host ships, keyed by their path under
`manual/`, and to the limits every plugin call runs within.

### `development::Manual::write`

```rust
pub async fn write(&self, registry: &Registry) -> Result<(), DevelopmentError>
```

Rewrite the manual for what the registry runs now.

### `development::Manual::follow`

```rust
pub fn follow(self: Arc<Self>, registry: Arc<Registry>) -> Following
```

Rewrite the manual after every view the registry publishes, until shutdown.

## enum `development::Operation`

`src/development/error.rs`

```rust
pub enum Operation {
    Apply,
    Deploy,
    Startup,
    Rename {
        name: enco_core::PluginName,
        to: enco_core::PluginName,
    },
    Rewind(super::RewindArguments),
    Scaffold,
    History,
    Status,
}
```

The development operation that failed; it decides what a failure left behind and how to
repeat the operation.

Implements:

```rust
impl Clone for Operation
impl Debug for Operation
```

## struct `development::PluginUnit`

`src/development/status.rs`

```rust
pub struct PluginUnit {
    pub running: Option<enco_kernel::PluginStatus>,
    pub registered: Option<RegisteredUnit>,
    pub home: Option<super::HomePlugin>,
}
```

Implements:

```rust
impl Default for PluginUnit
impl Serialize for PluginUnit
```

## struct `development::RegisteredUnit`

`src/development/status.rs`

```rust
pub struct RegisteredUnit {
    pub plugin: enco_kernel::RegisteredPlugin,
    pub earlier_sdk: bool,
    pub follow_rejection: Option<Vec<enco_kernel::Cause>>,
}
```

- `earlier_sdk` — No build of this host's SDK produced the registered code; the next registration rebuilds it.
- `follow_rejection` — Why registering this host's factory code was rejected; every registration retries it.

Implements:

```rust
impl Serialize for RegisteredUnit
```

## struct `development::RenameArguments`

`src/development/arguments.rs`

```rust
pub struct RenameArguments<R = enco_core::Reason> {
    pub name: enco_core::PluginName,
    pub to: enco_core::PluginName,
    pub reason: R,
}
```

- `name` — The plugin's current name.
- `reason` — Why the plugin is renamed, in one sentence; history shows it.

Implements:

```rust
impl<R: Clone> Clone for RenameArguments<R>
impl<R: Debug> Debug for RenameArguments<R>
impl<'de, R> Deserialize<'de> for RenameArguments<R> where R: Deserialize<'de>
impl<R: Eq> Eq for RenameArguments<R>
impl<R> JsonSchema for RenameArguments<R> where R: schemars::JsonSchema
impl<R: PartialEq> PartialEq for RenameArguments<R>
impl<R> Serialize for RenameArguments<R> where R: Serialize
```

## struct `development::RewindArguments`

`src/development/arguments.rs`

```rust
pub struct RewindArguments {
    pub registration: enco_core::RegistrationId,
    pub paths: Option<Vec<enco_core::IntentPath>>,
}
```

- `registration` — The registration whose versions are written back.
- `paths` — The paths to rewind, each `config.toml` or `plugins/<name>` with the plugin's current name.

Implements:

```rust
impl Clone for RewindArguments
impl Debug for RewindArguments
impl<'de> Deserialize<'de> for RewindArguments
impl Eq for RewindArguments
impl JsonSchema for RewindArguments
impl PartialEq for RewindArguments
impl Serialize for RewindArguments
```

## struct `development::RewindConflict`

`src/development/rewind.rs`

```rust
pub struct RewindConflict {
    pub path: enco_core::IntentPath,
    pub conflicts: Vec<super::tree::WriteConflict>,
}
```

Implements:

```rust
impl Debug for RewindConflict
impl Serialize for RewindConflict
```

## struct `development::Rewound`

`src/development/rewind.rs`

```rust
pub struct Rewound {
    pub registration: enco_core::RegistrationId,
    pub written: Vec<enco_core::IntentPath>,
    pub removed: Vec<enco_core::IntentPath>,
    pub unchanged: Vec<enco_core::IntentPath>,
}
```

### `development::Rewound::document`

`src/development/render/actions.rs`

```rust
pub fn document(&self) -> String
```

Implements:

```rust
impl Serialize for Rewound
```

## struct `development::ScaffoldArguments`

`src/development/arguments.rs`

```rust
pub struct ScaffoldArguments {
    pub name: enco_core::PluginName,
}
```

Implements:

```rust
impl Clone for ScaffoldArguments
impl Debug for ScaffoldArguments
impl<'de> Deserialize<'de> for ScaffoldArguments
impl Eq for ScaffoldArguments
impl JsonSchema for ScaffoldArguments
impl PartialEq for ScaffoldArguments
impl Serialize for ScaffoldArguments
```

## struct `development::Sdk`

`src/development/sdk.rs`

```rust
pub struct Sdk {
    /* private fields */
}
```

The plugin SDK sources embedded in the host binary, written to `sdk/` in each home.

### `development::Sdk::new`

```rust
pub fn new(files: &[(&str, &[u8])]) -> Result<Self, DevelopmentError>
```

Wrap the embedded files, rejecting a path that is not canonical and relative, or a bundle
whose `rust-toolchain.toml` names no channel.

### `development::Sdk::hash`

```rust
pub fn hash(&self) -> ContentHash
```

Content hash of the sources that can change a plugin build, the toolchain included; the
SDK's own `Cargo.lock` is left out.

### `development::Sdk::toolchain`

```rust
pub fn toolchain(&self) -> &str
```

The Rust toolchain every plugin build in a home uses, as rustup names it.

### `development::Sdk::file`

```rust
pub fn file(&self, path: &str) -> Option<&[u8]>
```

One bundled file, such as the manifest or the lock a new plugin starts from.

### `development::Sdk::lock`

```rust
pub fn lock(&self) -> Option<&[u8]>
```

The tested dependency versions a new plugin starts from.

### `development::Sdk::write`

```rust
pub async fn write(&self, home: &Path) -> Result<(), DevelopmentError>
```

Write the SDK to `sdk/` in `home`, replacing whatever is there, edits included.

Implements:

```rust
impl Clone for Sdk
```

## enum `development::SettingsComparison`

`src/development/home.rs`

```rust
pub enum SettingsComparison {
    Same,
    Changed,
    Unknown,
}
```

- `Unknown` — `config.toml` or its plugin tables cannot be read; the reason is reported for `config.toml`.

Implements:

```rust
impl Clone for SettingsComparison
impl Copy for SettingsComparison
impl Debug for SettingsComparison
impl Eq for SettingsComparison
impl PartialEq for SettingsComparison
impl Serialize for SettingsComparison
```

## struct `development::StatusArguments`

`src/development/call.rs`

```rust
pub struct StatusArguments {
}
```

Implements:

```rust
impl Clone for StatusArguments
impl Debug for StatusArguments
impl Eq for StatusArguments
impl PartialEq for StatusArguments
impl Serialize for StatusArguments
```

## enum `development::SuggestedArguments`

`src/development/call.rs`

```rust
pub enum SuggestedArguments {
    Development(DevelopmentCall<super::AgentReason>),
    Status(StatusArguments),
}
```

Implements:

```rust
impl Clone for SuggestedArguments
impl Debug for SuggestedArguments
impl Serialize for SuggestedArguments
```

## enum `development::ToolchainProblem`

`src/development/error.rs`

```rust
pub enum ToolchainProblem {
    Missing,
    Target,
    Release {
        found: String,
    },
}
```

Why the SDK's toolchain cannot build plugins on this machine.

- `Missing` — `rustc` fails under the pinned toolchain.
- `Target` — The toolchain has no standard library for `wasm32-wasip2`.
- `Release` — The `rustc` Cargo would use does not go through rustup, so it ignores the pinned toolchain.
  - `found` — The release that `rustc -vV` reports.

Implements:

```rust
impl Debug for ToolchainProblem
impl Display for ToolchainProblem
impl Error for ToolchainProblem
```

## struct `development::UnitDiff`

`src/development/history.rs`

```rust
pub struct UnitDiff {
    pub path: IntentPath,
    pub plugin: Option<PluginId>,
    pub files: Vec<FileDiff>,
    pub unavailable: Option<ContentUnavailable>,
}
```

Implements:

```rust
impl Serialize for UnitDiff
```

## fn `development::registration_document`

`src/development/render/actions.rs`

```rust
pub fn registration_document(registered: &enco_kernel::Registered) -> String
```

## const `development::APPLY`

`src/development/call.rs`

```rust
pub const APPLY: &str = "self_apply";
```

## const `development::HISTORY`

`src/development/call.rs`

```rust
pub const HISTORY: &str = "self_history";
```

## const `development::HISTORY_PAGE_SIZE`

`src/development/history.rs`

```rust
pub const HISTORY_PAGE_SIZE: usize = 20;
```

## const `development::MANUAL_DIRECTORY`

`src/development/manual.rs`

```rust
pub const DIRECTORY: &str = "manual";
```

The directory under the home that holds the manual.

## const `development::RENAME`

`src/development/call.rs`

```rust
pub const RENAME: &str = "plugin_rename";
```

## const `development::REWIND`

`src/development/call.rs`

```rust
pub const REWIND: &str = "self_rewind";
```

## const `development::SCAFFOLD`

`src/development/call.rs`

```rust
pub const SCAFFOLD: &str = "plugin_scaffold";
```

## const `development::STATUS`

`src/development/call.rs`

```rust
pub const STATUS: &str = "self_status";
```

# module `enco_host::memory`

`src/memory.rs`

- struct `memory::Memories` — same as `Memories`
- struct `memory::MemoryContextSource` — same as `MemoryContextSource`
- enum `memory::MemoryError` — same as `MemoryError`
- struct `memory::MemoryList` — same as `MemoryList`
- struct `memory::MemoryPaths` — same as `MemoryPaths`
- struct `memory::Recall` — same as `Recall`
- fn `memory::memory_tool_specs` — same as `memory_tool_specs`
- fn `memory::memory_tools` — same as `memory_tools`
- const `memory::TABLE_OWNER` — The same parser supplies admission and runtime consumers.

## const `memory::TABLE_OWNER`

`src/memory/settings.rs`

```rust
pub const TABLE_OWNER: enco_kernel::TableOwner = _;
```

The same parser supplies admission and runtime consumers.

# module `enco_host::status`

`src/status.rs`

- struct `status::Attention` — (no summary)
- enum `status::AttentionKind` — Fact templates receive only this value, so they cannot infer prose from the steps.
- enum `status::CallStep` — Each purpose describes exactly one effect, and carries the arguments of its call.
- enum `status::Divergence` — (no summary)
- enum `status::FixTarget` — What the Agent edits; each target is named by the facts that call for it.
- enum `status::GenerationFailure` — A recorded failure of a generation, attributed by what the generation introduced.
- struct `status::Node` — (no summary)
- enum `status::OwnerAction` — (no summary)
- struct `status::Repair` — Steps that repair one failure, and whether repeating the failed action is then worthwhile.
- struct `status::Status` — The application assembles this value once for both tools and CLI clients.
- enum `status::Step` — The purpose of each action is decided with the situation, never inferred by the renderer.
- struct `status::Steps` — At most two alternatives; each alternative is performed in order.
- enum `status::StopCause` — (no summary)
- fn `status::repair` — (no summary)

## struct `status::Attention`

`src/status/situation.rs`

```rust
pub struct Attention<'a> {
    pub kind: AttentionKind<'a>,
    pub next: super::Steps,
}
```

Implements:

```rust
impl<'a> Serialize for Attention<'a>
```

## enum `status::AttentionKind`

`src/status/situation.rs`

```rust
pub enum AttentionKind<'a> {
    New {
        path: enco_core::IntentPath,
    },
    Unregistered {
        path: enco_core::IntentPath,
        registration: enco_core::RegistrationId,
    },
    Unbuilt {
        path: enco_core::IntentPath,
        registration: enco_core::RegistrationId,
    },
    Removed {
        path: enco_core::IntentPath,
        registration: enco_core::RegistrationId,
        running: bool,
        factory: bool,
    },
    RolledBack {
        path: enco_core::IntentPath,
        running: enco_core::PluginActivation,
        registered: enco_core::PluginActivation,
        cause: GenerationFailure<'a>,
    },
    Diverged {
        path: enco_core::IntentPath,
        divergence: Divergence<'a>,
    },
    Stopped {
        path: enco_core::IntentPath,
        cause: StopCause<'a>,
    },
    ConfigurationDiverged {
        running: enco_core::ConfigurationRef,
        registered: enco_core::ConfigurationRef,
    },
    FollowRejected {
        path: enco_core::IntentPath,
        causes: &'a [enco_kernel::Cause],
        running: Option<enco_core::PluginActivation>,
    },
    UnservedUse {
        usage: &'a enco_kernel::UnservedUse,
    },
    InvalidTable {
        error: &'a enco_kernel::SettingsError,
    },
    Unreadable {
        path: enco_core::IntentPath,
        error: &'a crate::development::EntryError,
    },
    PluginsUnreadable {
        error: &'a crate::development::EntryError,
    },
    SafeMode,
    SchedulerStopped {
        reason: &'a str,
    },
    SessionStopped {
        name: &'a str,
        reason: &'a str,
    },
}
```

Fact templates receive only this value, so they cannot infer prose from the steps.

- `Unbuilt` — Source changed, and only a build can tell whether its code did.

Implements:

```rust
impl<'a> Serialize for AttentionKind<'a>
```

## enum `status::CallStep`

`src/status/next.rs`

```rust
pub enum CallStep {
    Review(crate::development::HistoryQuery),
    Compare(crate::development::HistoryQuery),
    List(Option<enco_core::RegistrationId>),
    Show(enco_core::RegistrationId),
    Register,
    RegisterRemoval,
    Discard {
        latest: enco_core::RegistrationId,
        paths: Vec<enco_core::IntentPath>,
    },
    KeepRunning(crate::development::RewindArguments),
    Restore(crate::development::RewindArguments),
    Rewind(crate::development::RewindArguments),
    Overwrite(crate::development::RewindArguments),
    RecordName {
        name: enco_core::PluginName,
        to: enco_core::PluginName,
    },
    Rename {
        name: enco_core::PluginName,
        to: enco_core::PluginName,
    },
    Recheck,
}
```

Each purpose describes exactly one effect, and carries the arguments of its call.

- `Discard` — Return these paths to the latest registration, dropping unregistered changes.
- `KeepRunning` — Return a path to the registration whose code or configuration runs.
- `Overwrite` — Return paths to a chosen registration, overwriting their unregistered changes.

### `status::CallStep::invocation`

```rust
pub fn invocation(&self) -> Invocation
```

Implements:

```rust
impl Clone for CallStep
impl Debug for CallStep
impl Eq for CallStep
impl From<CallStep> for Step
impl PartialEq for CallStep
impl Serialize for CallStep
```

## enum `status::Divergence`

`src/status/situation.rs`

```rust
pub enum Divergence<'a> {
    Name {
        running: &'a enco_core::PluginName,
        registered: &'a enco_core::PluginName,
        registration: enco_core::RegistrationId,
    },
    Code {
        running: enco_core::PluginActivation,
        registered: enco_core::PluginActivation,
    },
    Settings {
        running: enco_core::PluginActivation,
        registered: enco_core::PluginActivation,
    },
    Unregistered {
        running: enco_core::PluginActivation,
        removed_by: enco_core::RegistrationId,
    },
}
```

Implements:

```rust
impl<'a> Serialize for Divergence<'a>
```

## enum `status::FixTarget`

`src/status/next.rs`

```rust
pub enum FixTarget {
    Source {
        name: enco_core::PluginName,
    },
    PluginSettings {
        name: enco_core::PluginName,
    },
    Endpoint {
        name: String,
    },
    Table {
        name: String,
    },
    Entry {
        path: std::path::PathBuf,
    },
    Name {
        name: enco_core::PluginName,
    },
    References {
        name: enco_core::PluginName,
    },
    RecoveryTools,
    IgnoredEntries,
    RemoveDirectory {
        path: enco_core::IntentPath,
    },
    MoveDirectory {
        from: enco_core::IntentPath,
        to: enco_core::IntentPath,
    },
    SeparateNames {
        path: enco_core::IntentPath,
    },
}
```

What the Agent edits; each target is named by the facts that call for it.

Implements:

```rust
impl Clone for FixTarget
impl Debug for FixTarget
impl Eq for FixTarget
impl PartialEq for FixTarget
impl Serialize for FixTarget
```

## enum `status::GenerationFailure`

`src/status/situation.rs`

```rust
pub enum GenerationFailure<'a> {
    Code {
        failure: &'a enco_core::Failure,
    },
    Settings {
        failure: &'a enco_core::Failure,
    },
    Factory {
        failure: &'a enco_core::Failure,
    },
}
```

A recorded failure of a generation, attributed by what the generation introduced.

Implements:

```rust
impl<'a> Serialize for GenerationFailure<'a>
```

## struct `status::Node`

`src/status.rs`

```rust
pub struct Node {
    pub version: String,
    pub revision: String,
    pub home: std::path::PathBuf,
    pub execution: enco_kernel::Status,
    pub channels: Vec<crate::channel::ChannelStatus>,
}
```

Implements:

```rust
impl Serialize for Node
```

## enum `status::OwnerAction`

`src/status/next.rs`

```rust
pub enum OwnerAction {
    Restart,
    DisableSafeMode,
    RetryDeployment,
    FactoryCode {
        name: enco_core::PluginName,
    },
    Environment {
        name: enco_core::PluginName,
    },
    HostFault,
}
```

Implements:

```rust
impl Clone for OwnerAction
impl Debug for OwnerAction
impl Eq for OwnerAction
impl PartialEq for OwnerAction
impl Serialize for OwnerAction
```

## struct `status::Repair`

`src/status/next.rs`

```rust
pub struct Repair {
    pub steps: Vec<Step>,
    pub retry: bool,
}
```

Steps that repair one failure, and whether repeating the failed action is then worthwhile.

### `status::Repair::retry`

```rust
pub fn retry() -> Self
```

### `status::Repair::then_retry`

```rust
pub fn then_retry(steps: Vec<Step>) -> Self
```

### `status::Repair::only`

```rust
pub fn only(steps: Vec<Step>) -> Self
```

### `status::Repair::all`

```rust
pub fn all(repairs: impl IntoIterator<Item = Self>) -> Self
```

The repair of several failures, deduplicated, retried only when each allows it.

### `status::Repair::followed_by`

```rust
pub fn followed_by(self, retry: Step) -> Vec<Step>
```

## struct `status::Status`

`src/status.rs`

```rust
pub struct Status {
    pub development: crate::development::DevelopmentStatus,
    pub node: Node,
}
```

The application assembles this value once for both tools and CLI clients.

### `status::Status::document`

`src/development/render/status.rs`

```rust
pub fn document(&self) -> String
```

### `status::Status::attention`

```rust
pub fn attention(&self) -> Vec<Attention<'_>>
```

Situations are derived from the complete value, never cached or appended by callers.

Implements:

```rust
impl Serialize for Status
```

## enum `status::Step`

`src/status/next.rs`

```rust
pub enum Step {
    Call(CallStep),
    Fix(FixTarget),
    AskOwner(OwnerAction),
}
```

The purpose of each action is decided with the situation, never inferred by the renderer.

Implements:

```rust
impl Clone for Step
impl Debug for Step
impl Eq for Step
impl From<CallStep> for Step
impl PartialEq for Step
impl Serialize for Step
```

## struct `status::Steps`

`src/status/next.rs`

```rust
pub struct Steps {
    pub first: Vec<Step>,
    pub alternative: Option<Vec<Step>>,
}
```

At most two alternatives; each alternative is performed in order.

### `status::Steps::sequence`

```rust
pub fn sequence(first: Vec<Step>) -> Self
```

### `status::Steps::is_empty`

```rust
pub fn is_empty(&self) -> bool
```

Implements:

```rust
impl Clone for Steps
impl Debug for Steps
impl Serialize for Steps
```

## enum `status::StopCause`

`src/status/situation.rs`

```rust
pub enum StopCause<'a> {
    Failed {
        cause: GenerationFailure<'a>,
    },
    Unavailable {
        causes: &'a [enco_kernel::Cause],
    },
    NotRunning {
        registered: enco_core::PluginActivation,
    },
}
```

- `NotRunning` — A generation is registered, but none is selected to run.

Implements:

```rust
impl<'a> Serialize for StopCause<'a>
```

## fn `status::repair`

`src/status/next.rs`

```rust
pub fn repair(part: &enco_kernel::IntentPart) -> Repair
```

# module `enco_host::telegram`

`src/telegram.rs`

Telegram-specific configuration, transport evidence and text limits.

- struct `telegram::Telegram` — same as `Telegram`
- struct `telegram::TelegramChannel` — Follows the Telegram table, awaiting the old connection before starting the next one.
- const `telegram::TABLE_OWNER` — Telegram owns its table and declares no model endpoints.

## struct `telegram::TelegramChannel`

`src/telegram/supervisor.rs`

```rust
pub struct TelegramChannel {
    /* private fields */
}
```

Follows the Telegram table, awaiting the old connection before starting the next one.

### `telegram::TelegramChannel::start`

```rust
pub fn start(kernel: Arc<Kernel>, clock: Arc<dyn Clock>) -> Self
```

### `telegram::TelegramChannel::status`

```rust
pub async fn status(&self) -> Result<Vec<ChannelStatus>, KernelError>
```

### `telegram::TelegramChannel::shutdown`

```rust
pub async fn shutdown(&self) -> Result<(), ChannelError>
```

Implements:

```rust
impl Drop for TelegramChannel
```

## const `telegram::TABLE_OWNER`

`src/telegram.rs`

```rust
pub const TABLE_OWNER: enco_kernel::TableOwner = _;
```

Telegram owns its table and declares no model endpoints.

# module `enco_host::development::tree`

`src/development/tree.rs`

A directory as an exact value: file bytes by canonical relative path.

A source directory honors only the `.gitignore` files inside it, so its value is the same on
every machine. A write to it keeps ignored entries and `.git`, and is refused when one of them
is in the way, or when the write changes a `.gitignore` while ignored entries exist. A directory
Enco owns entirely, such as `sdk/`, is written exactly and loses everything else.

- struct `development::tree::FileTree` — Exact file bytes keyed by canonical paths relative to a directory.
- struct `development::tree::InvalidSourcePath` — (no summary)
- struct `development::tree::SourcePath` — A canonical UTF-8 path relative to a file tree, with `/` as its separator.
- enum `development::tree::WriteConflict` — A conflict is determined from the same directory observation as the planned writes.

## struct `development::tree::FileTree`

`src/development/tree.rs`

```rust
pub struct FileTree {
    /* private fields */
}
```

Exact file bytes keyed by canonical paths relative to a directory.

### `development::tree::FileTree::read`

```rust
pub async fn read(root: &Path) -> Result<Self, DevelopmentError>
```

Read source files using only ignore rules owned by this directory.

### `development::tree::FileTree::hash`

```rust
pub fn hash(&self) -> ContentHash
```

The hash of the canonical compact JSON manifest, with keys in byte order.

Implements:

```rust
impl Clone for FileTree
impl Default for FileTree
impl Eq for FileTree
impl PartialEq for FileTree
```

## struct `development::tree::InvalidSourcePath`

`src/development/tree/path.rs`

```rust
pub struct InvalidSourcePath(/* private field */);
```

Implements:

```rust
impl Debug for InvalidSourcePath
impl Display for InvalidSourcePath
impl Error for InvalidSourcePath
impl From<InvalidSourcePath> for DevelopmentError
```

## struct `development::tree::SourcePath`

`src/development/tree/path.rs`

```rust
pub struct SourcePath(/* private field */);
```

A canonical UTF-8 path relative to a file tree, with `/` as its separator.

### `development::tree::SourcePath::as_str`

```rust
pub fn as_str(&self) -> &str
```

Implements:

```rust
impl AsRef<Path> for SourcePath
impl Borrow<str> for SourcePath
impl Clone for SourcePath
impl Debug for SourcePath
impl<'de> Deserialize<'de> for SourcePath
impl Display for SourcePath
impl Eq for SourcePath
impl FromStr for SourcePath
impl Ord for SourcePath
impl PartialEq for SourcePath
impl PartialOrd for SourcePath
impl Serialize for SourcePath
```

## enum `development::tree::WriteConflict`

`src/development/tree/write.rs`

```rust
pub enum WriteConflict {
    Unregistered {
        path: std::path::PathBuf,
    },
    Occupied {
        path: std::path::PathBuf,
    },
    Ignored {
        path: std::path::PathBuf,
    },
    IgnoreRulesChanged {
        path: std::path::PathBuf,
    },
}
```

A conflict is determined from the same directory observation as the planned writes.

- `Occupied` — An ignored entry occupies a path the target writes.
- `Ignored` — An ignored entry keeps the directory from being removed.
- `IgnoreRulesChanged` — The target's ignore rules differ, so this ignored entry could become source.

Implements:

```rust
impl Clone for WriteConflict
impl Debug for WriteConflict
impl Eq for WriteConflict
impl PartialEq for WriteConflict
impl Serialize for WriteConflict
```
