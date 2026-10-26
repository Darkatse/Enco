<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_host`

`src/lib.rs`

Native storage, tools and context policies.

- module `channel` — Connections between a messaging protocol and Sessions.
- module `development` — Turns the plugin sources and `config.toml` in a home into registrations, and registered versions back into files.
- module `memory` — The owner's memories: a SQLite database holds them, and a derived vector index finds them.
- module `status` — Enco's state as `self_status` reports it, and what in it needs attention.
- struct `InstructionsContextSource` — Supplies the owner's current standing instructions once per Round.
- struct `Memories` — Sole entry point for memory writes and recall. The index never owns memory content.
- struct `MemoryContextSource` — Offers the pinned memories, and the memories recalled for a new message, to each Round.
- enum `MemoryError` — Why a memory operation failed.
- struct `MemoryList` — Every memory, and which of them the index does not cover yet.
- struct `MemoryPaths` — Where the memory database and its vector index live; `Memories` owns both.
- struct `Recall` — The memories recalled for a query: those the index found, and recent ones it does not cover yet.
- struct `SqliteStore` — Enco's store: the SQLite database, with the content files and node identity that go with it.
- struct `StorePaths` — Files owned by the persistence adapter.
- struct `SystemClock` — Host wall clock; Log order is assigned independently by the Session.
- fn `blob_path` — Address a blob without opening the store.
- fn `development_tool_specs` — The development tools' declarations, available before their handlers exist.
- fn `development_tools` — The development tools, running the same operations as the CLI.
- fn `memory_tool_specs` — The memory tools' declarations, available before their handlers exist.
- fn `memory_tools` — The memory tools, `memory_save`, `memory_update`, `memory_forget` and `memory_search`, acting on `memories`.
- fn `native_tools` — The native tools: the file tools, `message_send` and `shell_exec`, with paths resolved against `workspace`.
- fn `self_status_spec` — The declaration of `self_status`, whose handler belongs to the application.
- const `LIFELINE` — The repair tools that every reply must offer, unless its profile sets `requires_lifeline = false`.
- const `TABLE_OWNERS` — Configuration ownership for the host components assembled by the application.

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

Read the standing instructions from the file at `path`.

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

Open the database and the index at `paths`, then index the memories the index lacks, as far
as the embedding service allows.

### `Memories::save`

```rust
pub async fn save(&self, text: String, pinned: bool) -> Result<Memory, MemoryError>
```

Store a new memory and return it with its ID.

### `Memories::update`

```rust
pub async fn update(&self, id: MemoryId, text: Option<String>, pinned: Option<bool>) -> Result<Option<Memory>, MemoryError>
```

Replace a memory's text, its pin, or both; `None` when no memory has this ID.

### `Memories::forget`

```rust
pub async fn forget(&self, id: MemoryId) -> Result<bool, MemoryError>
```

Delete a memory for good; `false` when no memory has this ID.

### `Memories::pinned`

```rust
pub async fn pinned(&self) -> Result<Vec<Memory>, MemoryError>
```

Every pinned memory, oldest first.

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

Offers the pinned memories, and the memories recalled for a new message, to each Round.

### `MemoryContextSource::new`

```rust
pub fn new(memories: Arc<Memories>) -> Self
```

Offer what `memories` holds.

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

Why a memory operation failed.

- `Registry` — The registry failed while the embedding plugin's result was reported to it.
- `Authority` — The memory database failed; the text is its message.
- `SchemaVersion` — Another version of Enco wrote the memory database.
  - `found` — The version the database records.
  - `expected` — The version this Enco reads and writes.
- `Index` — The vector index cannot be read or written; deleting it while Enco is stopped rebuilds it.
- `Cancelled` — The caller cancelled the operation.

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

Every memory, and which of them the index does not cover yet.

- `memories` — Every memory, as the database holds it.
- `unindexed` — The memories the index does not cover yet.

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

Where the memory database and its vector index live; `Memories` owns both.

- `db` — The memory database, which holds every memory.
- `index` — The vector index, derived from the database.

## struct `Recall`

`src/memory.rs`

```rust
pub struct Recall {
    pub memories: Vec<Memory>,
    pub unindexed: Vec<Memory>,
    pub lexical_only: Option<Failure>,
}
```

The memories recalled for a query: those the index found, and recent ones it does not cover yet.

- `memories` — The memories the index found, as the database holds them now.
- `unindexed` — Recent memories the index does not cover yet, offered without ranking.
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

Enco's store: the SQLite database, with the content files and node identity that go with it.

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

The development tools' declarations, available before their handlers exist.

The registry needs the names before it binds the handlers.

## fn `development_tools`

`src/tools/development.rs`

```rust
pub fn development_tools(development: std::sync::Arc<crate::development::Development>, registry: std::sync::Arc<enco_kernel::Registry>) -> Result<Vec<std::sync::Arc<dyn Tool>>, SchemaError>
```

The development tools, running the same operations as the CLI.

## fn `memory_tool_specs`

`src/memory/tools.rs`

```rust
pub fn memory_tool_specs() -> Result<Vec<ToolSpec>, SchemaError>
```

The memory tools' declarations, available before their handlers exist.

## fn `memory_tools`

`src/memory/tools.rs`

```rust
pub fn memory_tools(memories: std::sync::Arc<super::Memories>) -> Result<Vec<std::sync::Arc<dyn Tool>>, SchemaError>
```

The memory tools, `memory_save`, `memory_update`, `memory_forget` and `memory_search`, acting
on `memories`.

## fn `native_tools`

`src/tools.rs`

```rust
pub fn native_tools(workspace: std::path::PathBuf) -> Result<Vec<std::sync::Arc<dyn Tool>>, enco_core::SchemaError>
```

The native tools: the file tools, `message_send` and `shell_exec`, with paths resolved against
`workspace`.

They share the workspace path rules and ordinary Tool dispatch.

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

The repair tools that every reply must offer, unless its profile sets
`requires_lifeline = false`.

They are the minimum the Agent needs to repair Enco, in safe mode and in ordinary operation.

## const `TABLE_OWNERS`

`src/lib.rs`

```rust
pub const TABLE_OWNERS: [enco_kernel::TableOwner; 2] = _;
```

Configuration ownership for the host components assembled by the application.

# module `enco_host::channel`

`src/channel.rs`

Connections between a messaging protocol and Sessions.

A connection owns its routing state; adapters own only protocol interpretation.

- struct `channel::Channel` — A running connection.
- enum `channel::ChannelError` — Why a connection stopped or could not start.
- struct `channel::ChannelStatus` — A connection's health, and the deliveries that failed for good.
- struct `channel::ChannelSupervisor` — Maintains the configured connections and joins each old owner before replacing it.
- struct `channel::ConnectionSettings` — One configured account and its host routing policy.
- const `channel::TABLE_OWNER` — Named connection tables, interpreted by the host and admitted by the registry.

## struct `channel::Channel`

`src/channel.rs`

```rust
pub struct Channel {
    /* private fields */
}
```

A running connection.

Shutdown signals the connection actor and all its helpers, then waits for them.

### `channel::Channel::start`

```rust
pub fn start(kernel: Arc<Kernel>, adapter: Arc<dyn Adapter>, settings: ConnectionSettings, generation: GenerationId, clock: Arc<dyn Clock>) -> Self
```

Start the connection; it carries addressed messages to Sessions and their replies back
until shutdown.

### `channel::Channel::status`

```rust
pub async fn status(&self) -> Result<ChannelStatus, KernelError>
```

The connection's health and its latest failed deliveries.

### `channel::Channel::shutdown`

```rust
pub async fn shutdown(&self) -> Result<(), ChannelError>
```

Stop the connection and wait until it and its helpers have stopped.

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
    Protocol(enco_kernel::ChannelCallError),
    Task(String),
    Unjoined(tokio::task::JoinError),
    Config(String),
}
```

Why a connection stopped or could not start.

- `Kernel` — The kernel failed to accept input or report Session facts.
- `State` — The stored connection state cannot be read.
- `Protocol` — The protocol failed in a way that stops the connection.
- `Task` — A task of the connection stopped unexpectedly.
- `Unjoined` — The owner did not complete cleanup, so its helpers may still be running.
- `Config` — The channel's configuration is invalid, such as a malformed bot token.

Implements:

```rust
impl Debug for ChannelError
impl Display for ChannelError
impl Error for ChannelError
impl From<ChannelCallError> for ChannelError
impl From<Error> for ChannelError
impl From<KernelError> for ChannelError
```

## struct `channel::ChannelStatus`

`src/channel.rs`

```rust
pub struct ChannelStatus {
    pub name: String,
    pub key: Option<String>,
    pub running: bool,
    pub stopped: Option<String>,
    pub failure_origin: Option<MappingOrigin>,
    pub menu_failure: Option<String>,
    pub recent_failures: Vec<enco_kernel::DeliveryStatus>,
}
```

A connection's health, and the deliveries that failed for good.

- `name` — The connection's name in the `channel` table.
- `key` — The account's state key, `<protocol>:<account>`, once the mapper identified it.
- `running` — Whether the connection is working now.
- `stopped` — Why it stopped, when it stopped on a failure.
- `failure_origin` — Recorded mapper call responsible for the stopped connection, when one exists.
- `menu_failure` — Why publishing the protocol command menu failed; the connection continues running.
- `recent_failures` — The latest deliveries that failed for good.

Implements:

```rust
impl Serialize for ChannelStatus
```

## struct `channel::ChannelSupervisor`

`src/channel/supervisor.rs`

```rust
pub struct ChannelSupervisor {
    /* private fields */
}
```

Maintains the configured connections and joins each old owner before replacing it.

### `channel::ChannelSupervisor::start`

```rust
pub fn start(kernel: Arc<Kernel>, store: Arc<dyn Store>, clock: Arc<dyn Clock>) -> Self
```

Follow the registry's active view, opening accounts through their fixed mapper generation.

### `channel::ChannelSupervisor::status`

```rust
pub async fn status(&self) -> Result<Vec<ChannelStatus>, KernelError>
```

Status for every configured connection, including account and menu failures.

### `channel::ChannelSupervisor::shutdown`

```rust
pub async fn shutdown(&self) -> Result<(), ChannelError>
```

Signal stop and wait until every connection and its helpers have stopped.

Implements:

```rust
impl Drop for ChannelSupervisor
```

## struct `channel::ConnectionSettings`

`src/channel/settings.rs`

```rust
pub struct ConnectionSettings {
    pub plugin: enco_core::PluginName,
    pub base_url: String,
    pub owner: String,
    pub secrets: std::collections::BTreeMap<String, String>,
    pub context: usize,
    pub group_profile: Option<String>,
    pub name: String,
}
```

One configured account and its host routing policy.

- `plugin` — Registered mapper plugin.
- `base_url` — Service origin, with an optional base path.
- `owner` — Protocol ID of the owner.
- `secrets` — Credential names mapped to environment variable names.
- `context` — Maximum number of earlier group messages quoted with the next input.
- `group_profile` — Profile for the Sessions this connection creates for groups; absent, it serves private
  chats only.
- `name` — Configuration name used to name new group Sessions.

Implements:

```rust
impl Clone for ConnectionSettings
impl<'de> Deserialize<'de> for ConnectionSettings
impl Eq for ConnectionSettings
impl PartialEq for ConnectionSettings
impl Serialize for ConnectionSettings
```

## const `channel::TABLE_OWNER`

`src/channel/settings.rs`

```rust
pub const TABLE_OWNER: enco_kernel::TableOwner = _;
```

Named connection tables, interpreted by the host and admitted by the registry.

# module `enco_host::development`

`src/development.rs`

Turns the plugin sources and `config.toml` in a home into registrations, and registered versions
back into files.

The owner and the Agent edit those files; what is registered and what runs live in Enco's store,
and only the registry changes them.

- module `development::tree` — A directory as an exact value: file bytes by canonical relative path.
- struct `development::AgentReason` — A suggested call's reason, which the Agent writes.
- struct `development::Applied` — The result of an apply: what was registered, and what each build did.
- struct `development::ApplyArguments` — The arguments of `self_apply`.
- enum `development::BuildFailure` — Why one plugin's source did not build.
- struct `development::BuildResult` — What happened to one plugin source during an apply.
- struct `development::Comparison` — How the files of two sides differ, and what to do about it.
- enum `development::ComparisonMode` — Which pair of sides a comparison has.
- struct `development::ConfigurationUnit` — The three layers of `config.toml`.
- enum `development::ContentFailure` — Why content cannot be read; other failures stop the comparison.
- struct `development::ContentUnavailable` — One side's content that cannot be read, and why.
- struct `development::Development` — Entry point for the plugin development tools in one home.
- enum `development::DevelopmentCall` — One parsed call of a development tool.
- enum `development::DevelopmentError` — Why a development operation failed.
- struct `development::DevelopmentStatus` — The three layers of every plugin and of `config.toml`, observed once.
- enum `development::DevelopmentTool` — A development tool that this crate implements; `self_status` belongs to the application.
- struct `development::EntryError` — A file or directory in the home that cannot be used as it is.
- enum `development::EntryProblem` — What is wrong with a file or directory in the home.
- struct `development::FactorySources` — The source of each factory plugin, as the host ships it.
- struct `development::Failed` — A failed development operation; its message says what failed, what was left behind, and what to do next.
- struct `development::FileDiff` — One file that differs.
- enum `development::FileDifference` — How a file differs between the two sides.
- struct `development::Following` — Keeps a manual in step with the registry until shutdown.
- struct `development::History` — The answer to a history request.
- enum `development::HistoryPoint` — One side of a comparison: what a registration accepted, or what the home holds now.
- enum `development::HistoryQuery` — What a history request asks for: a page of registrations, or a comparison of two sides.
- enum `development::HomeComparison` — How `config.toml` or a plugin directory in the home compares with what is registered.
- struct `development::HomePlugin` — How a plugin in the home compares with what is registered.
- struct `development::Invocation` — A suggested call, serialized only where a document or JSON result is written.
- struct `development::Manual` — Writes the manual of one home.
- enum `development::Operation` — The development operation that failed; it decides what a failure left behind and how to repeat the operation.
- struct `development::PluginUnit` — The three layers of one plugin; a layer is absent when the plugin is not in it.
- struct `development::RegisteredUnit` — What the latest registration holds for one plugin.
- struct `development::RenameArguments` — The arguments of `plugin_rename`.
- struct `development::RewindArguments` — The arguments of `self_rewind`.
- struct `development::RewindConflict` — What keeps a rewind from writing one path.
- struct `development::Rewound` — What a rewind wrote back into the home.
- struct `development::ScaffoldArguments` — The arguments of `plugin_scaffold`.
- struct `development::Sdk` — The plugin SDK sources embedded in the host binary, written to `sdk/` in each home.
- enum `development::SettingsComparison` — How a plugin's table in `config.toml` compares with the registered settings.
- struct `development::StatusArguments` — The arguments of `self_status`, which takes none.
- enum `development::SuggestedArguments` — The arguments of a suggested call.
- enum `development::ToolchainProblem` — Why the SDK's toolchain cannot build plugins on this machine.
- struct `development::UnitDiff` — How one unit differs between the two sides: `config.toml` or a plugin directory.
- fn `development::registration_document` — A registration as the Markdown document a tool result shows.
- fn `development::self_status_spec` — same as `self_status_spec`
- type `development::FactoryFiles` — One factory plugin's shipped files, as paths relative to its directory and their bytes.
- const `development::APPLY` — Name of the tool that registers every change in the home.
- const `development::HISTORY` — Name of the tool that lists registrations and compares versions of the home's files.
- const `development::HISTORY_PAGE_SIZE` — How many registrations one page of the history lists.
- const `development::MANUAL_DIRECTORY` — The directory under the home that holds the manual.
- const `development::RENAME` — Name of the tool that renames a plugin together with its directory.
- const `development::REWIND` — Name of the tool that writes an earlier registration's files back into the home.
- const `development::SCAFFOLD` — Name of the tool that creates a plugin directory in the home.
- const `development::STATUS` — Name of the tool that shows Enco's state and what needs attention.

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

The apply as the Markdown document a tool result shows.

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

The arguments of `self_apply`.

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

Why one plugin's source did not build.

- `Cargo` — A Cargo command failed.
  - `stage` — The command: `clippy`, `build` or `doc`.
  - `diagnostics` — Diagnostic excerpts, or the error output when Cargo reported none.
  - `log` — Where Cargo's complete output is stored.
- `NoArtifact` — Cargo succeeded but produced no Wasm component for the plugin; the path is the stored log.
- `EmptySettings` — The component does not load with an empty configuration, which every plugin must accept.
- `SourceChanged` — Files in the source directory changed while the build was reading them.
- `Toolchain` — The pinned Rust toolchain cannot build plugins on this machine.

  This is checked before Cargo runs. Installing a toolchain downloads it, so the owner repairs
  this.
  - `channel` — The toolchain the SDK pins, as rustup names it.
  - `problem` — What is missing or wrong.

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
- `log` — Path to Cargo's complete output, retained until the build directory is cleaned; absent on reuse.

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

How the files of two sides differ, and what to do about it.

- `from` — The side the comparison starts from; absent only before the first registration.
- `to` — The side the comparison ends at.
- `mode` — Which pair of sides this is, which decides the suggested steps.
- `units` — One entry for each unit that differs: `config.toml` or a plugin directory.
- `conflicting_paths` — Directories that two plugins would both need, so no rewind can write them.
- `next` — What to do about the differences, if anything.

  Actions include only paths addressable by the rewind tool.

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

Which pair of sides a comparison has.

- `HomeChanges` — The latest registration against the home: the edits not yet registered.
- `RewindPreview` — The home against a registration: what a rewind to that registration would write.
- `Registrations` — Any other pair, such as two registrations.

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

The three layers of `config.toml`.

- `running` — The configuration in use; it differs from the registered one after a rollback.
- `registered` — The configuration the latest registration selected.
- `home` — How `config.toml` in the home compares with the registered configuration.
- `invalid` — Tables in the running configuration that this host cannot interpret.
- `unavailable_uses` — Configured uses that no running plugin can serve now.

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

- `Missing` — Enco's store no longer has the content; stored blobs are removable history.
  - `hash` — The address of the missing content.
- `Corrupt` — The stored content cannot be read back.
  - `reason` — What is malformed.
- `Unreadable` — The files in the home cannot be read.

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

One side's content that cannot be read, and why.

- `point` — The side whose content cannot be read.
- `cause` — Why it cannot be read.

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

### `development::Development::rename`

`src/development/rename.rs`

```rust
pub async fn rename(&self, registry: &Registry, name: &PluginName, to: &PluginName, initiator: Initiator, cancel: &CancellationToken) -> Result<Registered, Failed>
```

Move `plugins/<name>` to `plugins/<to>` and register the new name in one operation.

### `development::Development::rewind`

`src/development/rewind.rs`

```rust
pub async fn rewind(&self, registry: &Registry, arguments: RewindArguments, cancel: &CancellationToken) -> Result<Rewound, Failed>
```

Write the files of `arguments.registration` back to the selected paths; nothing is
registered.

### `development::Development::scaffold`

`src/development/scaffold.rs`

```rust
pub async fn scaffold(&self, registry: &Registry, name: &PluginName) -> Result<Vec<PathBuf>, Failed>
```

Write a plugin directory without registering an identity or changing running code.

A factory plugin that follows the host starts from its shipped source: from the next
registration it is ordinary source and no longer follows. Any other free name starts from
the tool template.

### `development::Development::status`

`src/development/status.rs`

```rust
pub async fn status(&self, registry: &Registry) -> Result<DevelopmentStatus, Failed>
```

Inspect source and registry facts without building or changing either of them.

### `development::Development::new`

```rust
pub fn new(home: PathBuf, sdk: Sdk, factory: FactorySources, manual: Manual, store: Arc<SqliteStore>, runtime: Arc<dyn Runtime>) -> Arc<Self>
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

One parsed call of a development tool.

Suggestions use `AgentReason` where the Agent writes.

- `Apply` — A call of `self_apply`.
- `History` — A call of `self_history`.
- `Rewind` — A call of `self_rewind`.
- `Rename` — A call of `plugin_rename`.
- `Scaffold` — A call of `plugin_scaffold`.

### `development::DevelopmentCall::tool`

```rust
pub fn tool(&self) -> DevelopmentTool
```

The tool this call is for.

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

Why a development operation failed.

- `Entry` — A file or directory in the home cannot be used as it is.
- `Build` — A plugin's source did not build.
  - `name` — The plugin whose source did not build.
  - `failure` — How the build failed.
- `Registry` — The registry rejected the change or could not complete it.
- `Store` — Enco's store could not read or write.
- `Corrupt` — Content in Enco's store cannot be read back, such as a malformed file manifest.
- `Host` — The host's own machinery failed, such as starting a process or reading its embedded files.
- `NameTaken` — A new plugin was asked for under a name that is already registered or reserved.
  - `name` — The name asked for.
- `DirectoryExists` — The directory that a new or renamed plugin would take already exists.
  - `name` — The name whose directory exists.
- `NoSource` — A rename was asked for a plugin with no registered source directory to move, such as one
  that follows factory code.
  - `name` — The plugin asked to be renamed.
- `AmbiguousRename` — Registered sources disappeared while new directories appeared; a rename is ambiguous.
  - `missing` — Registered plugins whose directories are gone.
  - `added` — Directories under `plugins/` that no registration has.
  - `latest` — The latest registration, from which the missing directories can be restored.
- `Preparation` — Some sources or `config.toml` could not be prepared; each failure names its subject.
- `Moved` — The rename failed after its directory moved.
- `Partial` — The operation stopped after these file operations completed.
  - `written` — The files and directories written before it stopped.
  - `source` — Why it stopped.
- `RewindRefused` — A rewind would overwrite or remove something no registration holds; nothing was written.
  - `conflicts` — What stands in the way, for each path.
  - `selected` — Every path the rewind selected.
- `RewindPath` — A rewind named a plugin that neither the latest nor the target registration has.
  - `path` — The path as the rewind named it.
- `RewindCollision` — A rewind would write two plugins to the same directory.
  - `path` — The directory both would be written to.
- `RewindContent` — A rewind needs registered content that can no longer be read from Enco's store.
  - `path` — The path the content was to be written to.
  - `unavailable` — Why the content cannot be read.
- `WriteConflicts` — Writing a directory would destroy something the write does not own, such as an ignored file.
  - `path` — The directory being written.
  - `conflicts` — What stands in the way.
- `Cancelled` — The caller cancelled the operation before it finished.

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

- `registration` — The latest registration, which the registered layer reflects.
- `plugins` — Every plugin that runs, is registered, or has a directory.
- `configuration` — The three layers of `config.toml`.
- `recent_registrations` — The latest registrations, newest first.
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

A development tool that this crate implements; `self_status` belongs to the application.

- `Apply` — `self_apply`.
- `History` — `self_history`.
- `Rewind` — `self_rewind`.
- `Rename` — `plugin_rename`.
- `Scaffold` — `plugin_scaffold`.

### `development::DevelopmentTool::ALL`

```rust
const ALL: [Self; 5] = _;
```

Every development tool.

### `development::DevelopmentTool::name`

```rust
pub const fn name(self) -> &'static str
```

The name the model calls the tool by.

### `development::DevelopmentTool::named`

```rust
pub fn named(name: &str) -> Option<Self>
```

The tool called `name`, if there is one.

### `development::DevelopmentTool::parse`

```rust
pub fn parse(self, arguments: Map<String, Value>) -> Result<DevelopmentCall, Failure>
```

Parse the arguments of a call to this tool.

Execution uses this parser and no other.

### `development::DevelopmentTool::spec`

```rust
pub fn spec(self) -> Result<ToolSpec, SchemaError>
```

The tool's declaration: its name, description, argument schema and effect.

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

A file or directory in the home that cannot be used as it is.

- `path` — The file or directory.
- `problem` — What is wrong with it.

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

What is wrong with a file or directory in the home.

- `Io` — The operating system refused to read or write it; the text is its message.
- `NotDirectory` — A plugin's source is a file or a symbolic link where a directory must be.
- `NotRegular` — A source directory holds something other than files and directories, such as a symbolic
  link.
- `NotUtf8` — Its name is not valid UTF-8.
- `UnsupportedName` — Its name cannot be part of a source path.

  Source paths use `/` as their only separator, so names cannot contain `\\`.
- `NotText` — Its content is not UTF-8 text.
- `IgnoreRule` — A `.gitignore` rule cannot be parsed; the text is the parser's message.
- `NotPluginName` — A directory under `plugins/` whose name is not a valid plugin name.
- `Toml` — `config.toml` cannot be parsed; the text is the parser's message.
- `ReadmeMarkers` — A plugin's README has only one marker of its generated section, or both in the wrong order.

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

## struct `development::FactorySources`

`src/development/scaffold.rs`

```rust
pub struct FactorySources(/* private field */);
```

The source of each factory plugin, as the host ships it.

### `development::FactorySources::new`

```rust
pub fn new(sources: &[(&str, &FactoryFiles)]) -> Result<Self, DevelopmentError>
```

Read the embedded sources, keyed by the factory plugin's name.

Implements:

```rust
impl Default for FactorySources
```

## struct `development::Failed`

`src/development/error.rs`

```rust
pub struct Failed {
    pub operation: Operation,
    pub error: Box<DevelopmentError>,
}
```

A failed development operation; its message says what failed, what was left behind, and what
to do next.

- `operation` — The operation that failed, which decides how to repeat it.
- `error` — Why it failed.

### `development::Failed::next`

`src/development/error/next.rs`

```rust
pub fn next(&self) -> Steps
```

What to do about the failure: repair what it left behind, then repeat the operation.

### `development::Failed::code`

```rust
pub fn code(&self) -> &'static str
```

The stable classification of the error that failed the operation.

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

One file that differs.

- `path` — The file's path in the home, such as `config.toml` or `plugins/web/src/lib.rs`.
- `change` — How it differs.

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

How a file differs between the two sides.

- `Text` — The file is text on both sides.
  - `diff` — The change as a unified diff.
- `AddedEmpty` — An empty file was added.
- `RemovedEmpty` — An empty file was removed.
- `Binary` — One side is not text, so the change is not shown line by line.

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

The answer to a history request.

- `registrations` — A page of registrations, newest first, or the registration compared with the one before it.
- `before` — The cursor for older registrations, when more exist.
- `comparison` — The comparison, when one was asked for.
- `timezone` — The owner's time zone, in which registration times are shown.

### `development::History::document`

`src/development/render/history.rs`

```rust
pub fn document(&self) -> String
```

The history as the Markdown document a tool result shows.

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

One side of a comparison: what a registration accepted, or what the home holds now.

- `Registration` — The intent a registration accepted.
- `Home` — The files in the home now.

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

What a history request asks for: a page of registrations, or a comparison of two sides.

- `List` — A page of registrations, newest first.
  - `before` — Only registrations older than this one; the newest page when absent.
- `Compare` — The differences between the files of two sides.

  Without `from`, the home is compared with the latest registration, and a registration with
  the one before it.
  - `from` — The side the comparison starts from.
  - `to` — The side the comparison ends at.

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

How `config.toml` or a plugin directory in the home compares with what is registered.

Byte equality and meaning are distinct facts; reading a comparison never builds source.

- `Same` — The bytes are the registered ones.
- `TextOnly` — The bytes differ but mean the same, as when only a comment changed; registering them
  registers nothing.
- `Changed` — The bytes differ, and so does what they mean.
- `Unbuilt` — The bytes differ, and no build tells yet whether the code does.
- `Removed` — The registered directory is gone from the home.
- `Absent` — There is no directory and no registered source.
- `Unreadable` — The directory or file cannot be read.

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

How a plugin in the home compares with what is registered.

- `code` — Its source directory against the registered source.
- `settings` — Its table in `config.toml` against the registered settings.

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

- `tool` — The name of the tool to call.
- `arguments` — The arguments to call it with.
- `agent_supplies` — Arguments the Agent writes itself, such as `reason`; the suggestion holds placeholders.

### `development::Invocation::new`

```rust
pub fn new(call: DevelopmentCall<AgentReason>) -> Self
```

Suggest this call, naming the arguments the Agent writes itself.

### `development::Invocation::status`

```rust
pub fn status() -> Self
```

Suggest reading `self_status`.

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

- `Apply` — Registering the home as it is now, after building the plugins whose source changed.
- `Startup` — Restoring registered behavior as Enco starts, and registering a new home the first time.
- `Rename` — Renaming a plugin together with its directory.
  - `name` — The plugin's current name.
  - `to` — The name it was to take.
- `Rewind` — Writing parts of the home back to what an earlier registration registered.
- `Scaffold` — Creating a plugin directory from the template.
- `History` — Reading the registration history or comparing two points in it.
- `Status` — Reading how the home, the registrations and what runs compare.

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

The three layers of one plugin; a layer is absent when the plugin is not in it.

- `running` — What runs under this name.
- `registered` — What the latest registration holds for this name.
- `home` — How the plugin's directory in the home compares with what is registered.

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

What the latest registration holds for one plugin.

- `plugin` — The registered name, identity and code choice.
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

The arguments of `plugin_rename`.

- `name` — The plugin's current name.
- `to` — The new name.
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

The arguments of `self_rewind`.

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

What keeps a rewind from writing one path.

- `path` — The path the rewind would write.
- `conflicts` — What stands in the way there.

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

What a rewind wrote back into the home.

- `registration` — The registration whose files were written back.
- `written` — The paths whose files were written.
- `removed` — The plugin directories removed, because the registration had no plugin there.
- `unchanged` — The selected paths that already matched the registration.

### `development::Rewound::document`

`src/development/render/actions.rs`

```rust
pub fn document(&self) -> String
```

The rewind as the Markdown document a tool result shows.

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

The arguments of `plugin_scaffold`.

- `name` — The new plugin's name, or the name of a factory plugin whose source to take.

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

How a plugin's table in `config.toml` compares with the registered settings.

- `Same` — The table is the registered one.
- `Changed` — The table differs from the registered one.
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

The arguments of `self_status`, which takes none.

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

The arguments of a suggested call.

- `Development` — The arguments of a development tool, with placeholders where the Agent writes.
- `Status` — The arguments of `self_status`.

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

How one unit differs between the two sides: `config.toml` or a plugin directory.

- `path` — Where the unit is in the home.
- `plugin` — The plugin's identity, which stays the same across renames; absent for `config.toml` and
  for a directory no registration has.
- `files` — The files that differ; empty when one side cannot be read.
- `unavailable` — Why one side cannot be read, if it cannot.

Implements:

```rust
impl Serialize for UnitDiff
```

## fn `development::registration_document`

`src/development/render/actions.rs`

```rust
pub fn registration_document(registered: &enco_kernel::Registered) -> String
```

A registration as the Markdown document a tool result shows.

## type `development::FactoryFiles`

`src/development/scaffold.rs`

```rust
pub type FactoryFiles = [(&'static str, &'static [u8])];
```

One factory plugin's shipped files, as paths relative to its directory and their bytes.

## const `development::APPLY`

`src/development/call.rs`

```rust
pub const APPLY: &str = "self_apply";
```

Name of the tool that registers every change in the home.

## const `development::HISTORY`

`src/development/call.rs`

```rust
pub const HISTORY: &str = "self_history";
```

Name of the tool that lists registrations and compares versions of the home's files.

## const `development::HISTORY_PAGE_SIZE`

`src/development/history.rs`

```rust
pub const HISTORY_PAGE_SIZE: usize = 20;
```

How many registrations one page of the history lists.

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

Name of the tool that renames a plugin together with its directory.

## const `development::REWIND`

`src/development/call.rs`

```rust
pub const REWIND: &str = "self_rewind";
```

Name of the tool that writes an earlier registration's files back into the home.

## const `development::SCAFFOLD`

`src/development/call.rs`

```rust
pub const SCAFFOLD: &str = "plugin_scaffold";
```

Name of the tool that creates a plugin directory in the home.

## const `development::STATUS`

`src/development/call.rs`

```rust
pub const STATUS: &str = "self_status";
```

Name of the tool that shows Enco's state and what needs attention.

# module `enco_host::memory`

`src/memory.rs`

The owner's memories: a SQLite database holds them, and a derived vector index finds them.

The index never owns memory content, so deleting it loses nothing; it is rebuilt from the
database.

- struct `memory::Memories` — same as `Memories`
- struct `memory::MemoryContextSource` — same as `MemoryContextSource`
- enum `memory::MemoryError` — same as `MemoryError`
- struct `memory::MemoryList` — same as `MemoryList`
- struct `memory::MemoryPaths` — same as `MemoryPaths`
- struct `memory::Recall` — same as `Recall`
- fn `memory::memory_tool_specs` — same as `memory_tool_specs`
- fn `memory::memory_tools` — same as `memory_tools`
- const `memory::TABLE_OWNER` — The owner of the `[memory]` table, which declares the endpoints that memory uses.

## const `memory::TABLE_OWNER`

`src/memory/settings.rs`

```rust
pub const TABLE_OWNER: enco_kernel::TableOwner = _;
```

The owner of the `[memory]` table, which declares the endpoints that memory uses.

The same parser supplies admission and runtime consumers.

# module `enco_host::status`

`src/status.rs`

Enco's state as `self_status` reports it, and what in it needs attention.

Situations and their next steps are typed values derived from the complete state. The
application gathers the facts, and `Status::document` writes the text.

- struct `status::Attention` — One situation that needs attention, with the steps to take next.
- enum `status::AttentionKind` — The kind of situation, with only the facts that describe it.
- enum `status::CallStep` — A suggested tool call, named by the purpose it serves.
- enum `status::Divergence` — One way in which what runs for a plugin differs from what is registered.
- enum `status::FixTarget` — What the Agent edits; each target is named by the facts that call for it.
- enum `status::GenerationFailure` — A recorded failure of a generation, attributed by what the generation introduced.
- struct `status::Node` — This host: its build, its home, and what it executes.
- enum `status::OwnerAction` — Something only the owner can do; the Agent asks for it.
- struct `status::Status` — Enco's state now, as `self_status` and `enco status` report it.
- enum `status::Step` — One thing to do next: a tool call, a fix the Agent makes itself, or a request to the owner.
- struct `status::Steps` — The next steps for one situation or failure.
- enum `status::StopCause` — Why a plugin serves no calls.

## struct `status::Attention`

`src/status/situation.rs`

```rust
pub struct Attention<'a> {
    pub kind: AttentionKind<'a>,
    pub next: super::Steps,
}
```

One situation that needs attention, with the steps to take next.

- `kind` — What the situation is, with its facts; in JSON they sit beside `next`.
- `next` — What to do about it.

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

The kind of situation, with only the facts that describe it.

Fact templates receive only this value, so they cannot infer prose from the steps.

- `New` — A plugin directory in the home with no registered plugin under its name.
  - `path` — The plugin directory.
- `Unregistered` — The home has unregistered changes to code or configuration.
  - `path` — `config.toml` or the plugin directory that changed.
  - `registration` — The latest registration, which the home is compared with.
- `Unbuilt` — Source changed, and only a build can tell whether its code did.
  - `path` — The plugin directory whose source changed.
  - `registration` — The latest registration, which the home is compared with.
- `Removed` — A registered plugin's directory was deleted from the home, and the deletion is not
  registered.
  - `path` — The deleted plugin directory.
  - `registration` — Registration that registered the deleted source.
  - `running` — Whether the plugin still serves calls.
  - `factory` — Whether the host has factory code for the plugin, which runs once the removal is
    registered.
- `RolledBack` — The registered generation failed, and the plugin runs an older one instead.
  - `path` — The plugin directory.
  - `running` — The older generation that runs.
  - `registered` — The registered generation, which failed.
  - `cause` — The failure, by what the failed generation introduced.
- `Diverged` — What runs for the plugin differs from what is registered in one respect.
  - `path` — The plugin directory.
  - `divergence` — How what runs differs.
- `Stopped` — The plugin serves no calls.
  - `path` — The plugin directory.
  - `cause` — Why it serves no calls.
- `ConfigurationDiverged` — The running `config.toml` is not the registered one, as after a rollback.
  - `running` — The configuration that runs.
  - `registered` — The configuration the latest registration selected.
- `FollowRejected` — Registering this host's factory code for the plugin was rejected; every registration
  retries it.
  - `path` — The plugin directory.
  - `causes` — Why the registration was rejected.
  - `running` — The generation the plugin keeps running; absent when it serves no calls.
- `UnservedUse` — A configured use cannot be served, for a reason other than its plugin being unable to run.
  - `usage` — The use, the endpoint it names, and why it cannot be served.
- `InvalidTable` — A table of the running configuration that this host cannot interpret; what it configures
  stays stopped.
  - `error` — The table and what is wrong with it.
- `Unreadable` — `config.toml` or a plugin directory cannot be read, so it cannot be compared with what is
  registered.
  - `path` — `config.toml` or the plugin directory.
  - `error` — The file or directory that failed, and why.
- `PluginsUnreadable` — The `plugins` directory cannot be listed, so no plugin's home layer is known.
  - `error` — The file or directory that failed, and why.
- `SafeMode` — The owner turned safe mode on: only recovery tools are offered, and plugins with factory
  code run it.
- `SchedulerStopped` — The Scheduler stopped after a failure, and reminders are not delivered.
  - `reason` — The failure that stopped it.
- `SessionStopped` — A Session stopped after a storage failure, and messages to it are rejected.
  - `name` — The Session's name.
  - `reason` — The storage failure that stopped it.

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

A suggested tool call, named by the purpose it serves.

Each purpose describes exactly one effect, and carries the arguments of its call. In JSON,
`call` names the purpose, beside the `tool`, `arguments` and `agent_supplies` of `invocation`.

- `Review` — Read the unregistered changes before registering them.
- `Compare` — Compare two versions, such as the running configuration and the registered one.
- `List` — List registrations, newest first; with a number, only those older than it.
- `Show` — Show one registration in full, with its diff from the registration before it.
- `Register` — Register every change in the home as one registration.
- `RegisterRemoval` — Register the home after a plugin directory was deleted, so the deletion takes effect.
- `Discard` — Return these paths to the latest registration, dropping unregistered changes.
  - `latest` — The latest registration, whose versions are written back.
  - `paths` — The paths whose unregistered changes are dropped.
- `KeepRunning` — Return a path to the registration whose code or configuration runs.
- `Restore` — Bring deleted plugin directories back from a registration that holds them.
- `Rewind` — Write an earlier registration's versions back into the home.
- `Overwrite` — Return paths to a chosen registration, overwriting their unregistered changes.
- `RecordName` — Rename the plugin to the name it runs under, so the registered name matches it.
  - `name` — The registered name.
  - `to` — The name the plugin runs under.
- `Rename` — Rename a plugin together with its directory, and register the new name.
  - `name` — The plugin's current name.
  - `to` — The new name.
- `Recheck` — Read the status again, to see the state after a change.

### `status::CallStep::invocation`

```rust
pub fn invocation(&self) -> Invocation
```

The tool call for this step; `agent_supplies` names the arguments the Agent writes itself.

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

One way in which what runs for a plugin differs from what is registered.

- `Name` — The plugin runs under a name from an earlier registration.
  - `running` — The name it runs under.
  - `registered` — The registered name.
  - `registration` — Registration that gave the registered name.
- `Code` — The plugin runs code other than the registered code, which is not marked failed.
  - `running` — The generation that runs.
  - `registered` — The registered generation.
- `Settings` — The plugin runs settings other than the registered settings, which are not marked failed.
  - `running` — The generation that runs.
  - `registered` — The registered generation.
- `Unregistered` — A registration removed the plugin, but mechanical recovery, such as an owner rollback,
  runs it again.
  - `running` — The generation that runs.
  - `removed_by` — Registration that removed the plugin.

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

- `Source` — The plugin's source in `plugins/<name>`.
  - `name` — Plugin whose source needs fixing.
- `PluginSettings` — The plugin's `[plugin.<name>]` table in `config.toml`.
  - `name` — Plugin whose settings need fixing.
- `Endpoint` — The `[endpoint.<name>]` table in `config.toml`.
  - `name` — The endpoint's name.
- `Table` — A table of `config.toml`, such as `[memory]` or `[endpoint.chat]`.
  - `name` — Table name without brackets, such as `memory` or `endpoint.chat`.
- `Entry` — A file or directory in the home that cannot be used as it is.
  - `path` — Path of the file or directory.
- `Name` — A requested plugin name that cannot be used, to replace with another.
  - `name` — The name that cannot be used.
- `References` — References to a plugin name in `config.toml`, to remove.
  - `name` — The plugin name still referenced.
- `RecoveryTools` — A repair made with the recovery tools, the only tools offered in safe mode.
- `IgnoredEntries` — Ignored entries, listed by the refused rewind, that must move out of the paths it writes.
- `RemoveDirectory` — A plugin directory to delete from the home.
  - `path` — The directory to delete.
- `MoveDirectory` — A plugin directory to move back under its registered name.
  - `from` — Where the directory is now.
  - `to` — Where it belongs, under the registered name.
- `SeparateNames` — A directory a rewind would write two plugins to; the plugin now there needs a new name.
  - `path` — The directory both plugins would be written to.

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

- `Code` — The generation introduced code from a plugin directory.
  - `failure` — The failure recorded for the generation.
- `Settings` — The generation introduced new settings and reuses existing code.
  - `failure` — The failure recorded for the generation.
- `Factory` — The generation runs the host's factory code, which only a host update changes.
  - `failure` — The failure recorded for the generation.

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

This host: its build, its home, and what it executes.

- `version` — Release version of the host, such as `0.1.0`.
- `revision` — Git commit the host was built from; a build with uncommitted edits shows the same commit.
- `home` — The Enco home directory this host runs from.
- `execution` — Execution state from the kernel, such as safe mode, Sessions and the Scheduler; in JSON
  its fields sit directly in `node`.
- `channels` — Configured channels, such as Telegram, with their health and recent delivery failures.

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
    FactoryCode {
        name: enco_core::PluginName,
    },
    Environment {
        name: enco_core::PluginName,
    },
    HostFault,
}
```

Something only the owner can do; the Agent asks for it.

- `Restart` — Restart Enco; the Agent cannot restart the process it runs in.
- `DisableSafeMode` — Turn safe mode off with `enco safe-mode off`.
- `FactoryCode` — Repair the host's factory code for a plugin; only a host update changes it.
  - `name` — Plugin whose factory code is at fault.
- `Environment` — Provide what a plugin needs from outside the home, such as a secret value or a toolchain.
  - `name` — Plugin that lacks the requirement.
- `HostFault` — Look into a failure of the host itself, which the Agent cannot repair.

Implements:

```rust
impl Clone for OwnerAction
impl Debug for OwnerAction
impl Eq for OwnerAction
impl PartialEq for OwnerAction
impl Serialize for OwnerAction
```

## struct `status::Status`

`src/status.rs`

```rust
pub struct Status {
    pub development: crate::development::DevelopmentStatus,
    pub node: Node,
}
```

Enco's state now, as `self_status` and `enco status` report it.

The application assembles this value once for both tools and CLI clients. In JSON, the
development facts sit at the top level, beside `node` and the derived `needs_attention`.

- `development` — The three layers of every plugin and of `config.toml`, with recent registrations.
- `node` — This host and what it executes.

### `status::Status::document`

`src/development/render/status.rs`

```rust
pub fn document(&self) -> String
```

The status as the Markdown document `self_status` shows.

### `status::Status::attention`

```rust
pub fn attention(&self) -> Vec<Attention<'_>>
```

Every situation that needs attention now, each with its next steps.

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

One thing to do next: a tool call, a fix the Agent makes itself, or a request to the owner.

The purpose of each action is decided with the situation, never inferred by the renderer.

- `Call` — A tool call, with the arguments to pass.
- `Fix` — Something the Agent fixes itself, such as a file in the home.
- `AskOwner` — Something only the owner can do; the Agent asks for it.

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

The next steps for one situation or failure.

At most two alternatives; each alternative is performed in order.

- `first` — The steps to take, in order.
- `alternative` — Steps to take instead of `first`, in order; absent when there is no other choice.

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

Why a plugin serves no calls.

- `Failed` — The registered generation failed, and no older generation can run.
  - `cause` — The failure, by what the failed generation introduced.
- `Unavailable` — A generation is selected, but this host cannot serve it.
  - `causes` — Why this host cannot serve it.
- `NotRunning` — A generation is registered, but none is selected to run.
  - `registered` — The registered generation.

Implements:

```rust
impl<'a> Serialize for StopCause<'a>
```

# module `enco_host::development::tree`

`src/development/tree.rs`

A directory as an exact value: file bytes by canonical relative path.

A source directory honors only the `.gitignore` files inside it, so its value is the same on
every machine. A write to it keeps ignored entries and `.git`, and is refused when one of them
is in the way, or when the write changes a `.gitignore` while ignored entries exist. A directory
Enco owns entirely, such as `sdk/`, is written exactly and loses everything else.

- struct `development::tree::FileTree` — Exact file bytes keyed by canonical paths relative to a directory.
- struct `development::tree::InvalidSourcePath` — Text that is not a source path: an empty, `.` or `..` part, a `\\`, or an absolute path.
- struct `development::tree::SourcePath` — A canonical UTF-8 path relative to a file tree, with `/` as its separator.
- enum `development::tree::WriteConflict` — What stands in the way of writing a directory.

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

Text that is not a source path: an empty, `.` or `..` part, a `\\`, or an absolute path.

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

The path as text, with `/` between its parts.

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

What stands in the way of writing a directory.

A conflict is determined from the same directory observation as the planned writes.

- `Unregistered` — The file holds an edit that is not registered, which the write would overwrite.
  - `path` — The edited file.
- `Occupied` — An ignored entry occupies a path the target writes.
  - `path` — The ignored entry.
- `Ignored` — An ignored entry keeps the directory from being removed.
  - `path` — The ignored entry.
- `IgnoreRulesChanged` — The target's ignore rules differ, so this ignored entry could become source.
  - `path` — The ignored entry.

Implements:

```rust
impl Clone for WriteConflict
impl Debug for WriteConflict
impl Eq for WriteConflict
impl PartialEq for WriteConflict
impl Serialize for WriteConflict
```
