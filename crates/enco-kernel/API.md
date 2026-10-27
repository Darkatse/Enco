<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_kernel`

`src/lib.rs`

Session execution through explicit ports.

- struct `Acceptance` — What an atomic acceptance committed.
- enum `Accepted` — Whether an Event was newly accepted.
- struct `Activation` — One selected generation and where it came from; the running and registered layers share it.
- struct `ActivationWrite` — Mechanical recovery moves references; it never creates new intent.
- struct `Active` — The generation that runs under one name in ordinary operation.
- struct `ActiveConfig` — The active configuration generation: registered content plus the declarations derived from it.
- enum `Answer` — Finite probabilities in [0, 1], or a refusal for this question. Choice and score distributions follow label order and sum to one, allowing rounding.
- enum `ArtifactSource` — Newly built bytes or a content address already known to the store.
- enum `AttachmentContent` — The result of reading an attachment.
- struct `BoundComposer` — A composer generation paired with options accepted by that generation's schema.
- struct `Budget` — Context limits supplied by configuration.
- struct `CallContext` — Invocation identity, cooperative cancellation and the inline result budget.
- enum `Cause` — A failure attributed to the part of the intent it concerns.
- enum `ChannelAction` — The meaning of an incoming message, independent of protocol command syntax.
- struct `ChannelActivity` — A Run's current progress, containing only public tool names and outcomes.
- struct `ChannelAttachment` — An image still held by the protocol.
- enum `ChannelCallError` — A mapper or protocol failure, distinct from failing to record its evidence.
- struct `ChannelCommand` — A command offered by the host in protocol menus.
- enum `ChannelCommandKind` — The commands understood by the connection owner.
- struct `ChannelContent` — Text and attachments before or after fetching.
- struct `ChannelFault` — A protocol failure and the facts that determine retry and settlement.
- struct `ChannelIdentity` — Stable account identity declared by a mapper.
- struct `ChannelInput` — An incoming message, before host authorization or attachment fetching.
- struct `ChannelMessage` — One protocol message, its acceptance key and recorded origin.
- enum `ChannelReading` — One arrival's messages and protocol checkpoint, delivered in receive order.
- struct `ChannelSent` — The confirmed result of sending one part.
- struct `ChannelSettings` — The service and secret names chosen for one connection; values remain in transport.
- struct `ChannelToolActivity` — One tool call visible in a Run's progress view.
- enum `ChannelToolState` — A tool's current state in a progress view.
- struct `CodeProvenance` — Where a generation's code came from; settings-only generations follow their code generation.
- enum `CodeSource` — How a generation's code entered the registry.
- struct `Commit` — An atomic append of Log facts and consumption of their corresponding Inbox rows.
- struct `Completion` — Provider output with host-assigned call identities.
- struct `CompletionEndpoint` — A profile's endpoint with the context limits required by completion.
- struct `ComposeInput` — Fixed Round inputs, with the selected budgets and recorded recovery facts for this composition.
- struct `ComposerSettings` — A profile's selected composer and its invocation options.
- enum `Composition` — A ready reply request or a request to summarize earlier complete Rounds first.
- enum `CompositionItem` — Text written by a composer, or a reference to a recorded message.
- struct `CompositionPlan` — Composer output before the kernel freezes the Round's tool definitions with it.
- struct `ConfigStatus` — Configuration is one intent document; a rollback changes only its active reference.
- enum `ConfigTable` — The structural unit owned by one configuration declaration.
- struct `ConfigurationSource` — Parsed configuration paired with the exact input retained by the host.
- struct `ConnectionWrite` — State returned to the connection owner for atomic acceptance.
- struct `ConsumedMessage` — Where a channel message became a Session's input.
- struct `ContextError` — A context source failed to supply its authoritative content.
- struct `ContextQuery` — Shared input to a Round's context sources.
- struct `Credential` — A declared credential; transport validates placement and alone reads its value.
- struct `DeliveryStatus` — A delivery failure with the Session name resolved before presentation.
- enum `Difference` — One layer in which a registered plugin runs otherwise than registered.
- struct `Endpoint` — A resolved model service, independent of its caller and code generation.
- struct `EndpointFailure` — A candidate and the reason the kernel passed it over.
- struct `EndpointSpec` — A named model service; each user checks the facts it needs.
- struct `Endpoints` — An ordered, nonempty set of completion candidates accepted by configuration parsing.
- struct `Export` — An invocation's immutable association between code and registry identity.
- struct `Exports` — Derived routing view. Holding it does not prevent the registry from publishing a new view.
- struct `FactoryPlugin` — A plugin supplied with the host binary.
- struct `FactoryStatus` — Current host-owned recovery code and whether registered intent needs to follow it.
- struct `HeaderPlacement` — A request header carrying a credential.
- enum `ImageError` — Image ingestion either rejects its input or reports a storage failure.
- struct `Inspection` — An Attempt reconstructed exclusively from its recorded plan and Log prefix.
- enum `IntentPart` — The part of the owner's intent that a cause points to.
- struct `Kernel` — Single-node execution owner. Sessions serialize their own Log writes.
- struct `KernelDeps` — Ports assembled once by the application; no service locator is involved.
- enum `KernelError` — Public command failure; Round failures are recorded in the Log instead.
- struct `KernelSettings` — Parsed kernel tables, sampled by each consumer at its own boundary.
- struct `Label` — An option or score level. Names are unique within a question.
- enum `LoadError` — Why a component cannot be loaded, attributed to its code or to the settings it was given.
- struct `Loaded` — Exports returned by loading a component. Each invocation owns its own runtime state.
- struct `LoadedComposer` — A composer's option declaration and invocation, loaded from the same content.
- struct `LoadedTools` — Tool declarations are evaluated once with the generation's accepted configuration.
- struct `LocatedImage` — An image reference with the node-local address supplied by its store.
- enum `Location` — Where transport alone writes a credential value.
- enum `Need` — A use's interface and behavior requirements, shared by wiring checks and real probes.
- struct `NewSession` — A Session created together with the input that first reaches it.
- struct `NodeRecord` — Persistent node identity and recovery policy.
- struct `OfferedMemories` — The memories offered for one Round, and what could not be offered.
- struct `Placement` — One credential recipient and the location populated for that recipient.
- struct `PlanWrite` — An immutable request plan whose complete content address is computed once before committing.
- enum `PluginChange` — One plugin's contribution to an atomic registration.
- enum `PluginChangeKind` — Accepted changes to one plugin; each variant carries only its own facts.
- struct `PluginStatus` — What runs for one plugin, and how that differs from its registered intent.
- struct `Probe` — A successful real call made while applying configuration.
- struct `Profile` — Session policy sampled once at the start of a Round.
- enum `Proposed` — What a candidate generation changes, which decides the part its failure concerns.
- struct `ProviderRequest` — A canonical model request. Session requests resolve a frozen, recorded plan; registry probes contain only fixed validation content.
- struct `Question` — Instructions and the allowed answer shape for one independent judgment.
- enum `QuestionKind` — A predicate, unordered alternatives, or score levels ordered from low to high.
- enum `Referrer` — What in the configuration refers to a plugin name.
- struct `Registered` — The result of one accepted registration, with the evidence gathered before acceptance.
- struct `RegisteredIntent` — Only the facts needed to compare and prepare new intent.
- struct `RegisteredPlugin` — An accepted name, plus the code choice and current host implementation available to it.
- struct `RegisteredSource` — An explicitly registered code choice, independent of mechanical recovery.
- struct `RegistrationBase` — An accepted history position always has a complete configuration.
- struct `RegistrationDetails` — A bounded description of one accepted registration, without unrelated history.
- struct `RegistrationRequest` — One intentional change. All supplied parts are accepted or rejected together.
- struct `RegistrationWrite` — The sole committer assigns identities before handing a complete write to the store.
- struct `Registry` — Routing is published only after the durable mutation commits.
- struct `RegistryDeps` — Durable registry dependencies assembled by the application.
- enum `RegistryError` — An explicit registry operation could not complete.
- struct `RegistryState` — Durable facts; compiled code and admission are projections of this history.
- struct `RegistryStatus` — Plugin and configuration facts sampled under the same committer.
- struct `ReplyTarget` — The reply model the kernel selected, and why earlier candidates were passed over.
- struct `RewindScope` — The paths a rewind between two accepted states can address, decided once for every caller.
- struct `RewindSelection` — The configuration file and plugin directories one rewind writes.
- struct `Rollback` — A completed owner rollback, which may stop plugins when no recoverable code remains.
- enum `Running` — The running layer of one plugin.
- enum `SafeMode` — The reason for the Round's fixed recovery mode.
- enum `ScheduleError` — Schedule validation or persistence failure.
- struct `Scheduled` — An active schedule with its derived next occurrence, for tools and administration.
- struct `Schedules` — Commands to the sole owner of durable schedules.
- enum `ScopeError` — A requested rewind that the scope cannot address.
- struct `SessionStatus` — Live execution status, separate from durable Session facts.
- struct `SettingsError` — A configuration error attributed to the table that owns it.
- enum `Standing` — How the running layer of one plugin stands against its registered layer.
- struct `Status` — Current node and Session execution state.
- enum `StoreError` — A failed storage operation or commit precondition.
- struct `TableOwner` — A host-owned group of tables and its pure declaration function.
- enum `ToolOwner` — Who else exports a conflicting tool name.
- struct `Transcript` — Model-facing projection of immutable Log facts.
- struct `TranscriptItem` — One canonical message and its immutable source.
- enum `UnitChange` — One path's accepted change, named at the registration where it happened.
- struct `UnservedUse` — One current configured use that cannot be served by the selected exports.
- struct `Use` — One configured caller and the invocation it requires.
- enum `UseRef` — The configuration entry a use names, without copying its invocation parameters.
- enum `UseTarget` — The configured invocation, preserving the facts its actual interface consumes.
- enum `Verdict` — One completed invocation's verdict, before caller-specific result handling.
- struct `View` — The one published snapshot of routing and registered configuration.
- trait `Channel` — Opens accounts through one loaded, pure protocol mapper.
- trait `ChannelAdapter` — One identified account, using the same generation until it is stopped.
- trait `Clock` — Time boundary used for reminders and observation, never Log ordering.
- trait `Composer` — Pure policy boundary: identical inputs must produce identical plans.
- trait `ContextSource` — Supplies one owner's facts for a Round, in that owner's type; never called in safe mode.
- trait `Decision` — Typed judgments consumed by host policies, independently of Session completion.
- trait `Embedding` — Embedding boundary consumed by host memory, independently of completion.
- trait `Lifecycle` — Component lifecycle shared by all exported interfaces.
- trait `Provider` — Completion boundary. Implementations own timeouts and per-invocation state.
- trait `Runtime` — Compile and describe component bytes without knowing registry identities or routing.
- trait `Store` — Persistence boundary; only the owning actor may commit its state.
- trait `Tool` — An executable capability; dispatch is independent of tool identity.
- trait `Tools` — A plugin's tools interface, independent of the registry-assigned code identity.
- fn `builtin_tool_specs` — Native declarations available before their stateful handlers are constructed.
- fn `parse_table` — Parse one owner's table without interpreting other tables.
- fn `plugin_settings` — Parse plugin settings using the same ownership boundary as registration.
- type `ComposerOptions` — A profile's composer options: a JSON object whose shape the composer's schema declares.
- const `ENDPOINT_TABLE` — Named endpoint entries: `[endpoint.<name>]`.
- const `IMAGE_MAX_BYTES` — Maximum original bytes accepted for one image.
- const `PLUGIN_SETTINGS_TABLE` — The registry owns the named settings sections of plugins.
- const `PROFILE_TABLE` — Named profile entries: `[profile.<name>]`.
- const `RECENT_REGISTRATIONS` — The bounded registration history shown with current state.
- const `TRIAL_CALLS` — Successful trial calls required before a generation becomes a rollback target.

## struct `Acceptance`

`src/ports/store.rs`

```rust
pub struct Acceptance {
    pub events: Vec<Accepted>,
    pub session: Option<SessionRecord>,
}
```

What an atomic acceptance committed.

- `events` — Event results in input order.
- `session` — The Session created by this acceptance, as stored.

## enum `Accepted`

`src/ports/store.rs`

```rust
pub enum Accepted {
    New,
    Duplicate,
}
```

Whether an Event was newly accepted.

- `New` — Accepted for the first time.
- `Duplicate` — The Event ID was already accepted.

Implements:

```rust
impl Clone for Accepted
impl Copy for Accepted
impl Debug for Accepted
impl Eq for Accepted
impl PartialEq for Accepted
```

## struct `Activation`

`src/registry/intent.rs`

```rust
pub struct Activation {
    pub generation: GenerationRecord,
    pub registration: Option<RegistrationId>,
    pub code: CodeProvenance,
}
```

One selected generation and where it came from; the running and registered layers share it.

- `generation` — Selected generation and its current outcome.
- `registration` — Registration that selected it; absent for the host's factory default.
- `code` — Where the generation's code came from.

Implements:

```rust
impl Clone for Activation
impl Serialize for Activation
```

## struct `ActivationWrite`

`src/ports/store/registry.rs`

```rust
pub struct ActivationWrite {
    pub plugins: Vec<PluginRecord>,
    pub configuration: Option<GenerationId>,
    pub failed: Vec<(GenerationId, Failure)>,
    pub event: Option<Event>,
}
```

Mechanical recovery moves references; it never creates new intent.

- `plugins` — Final runtime references for affected identities.
- `configuration` — Configuration to activate, or leave unchanged when absent.
- `failed` — Attributable failures observed while recovering.
- `event` — Optional Session input committed with automatic recovery.

Implements:

```rust
impl Clone for ActivationWrite
impl Default for ActivationWrite
```

## struct `Active`

`src/registry/exports.rs`

```rust
pub struct Active<'a> {
    pub name: &'a PluginName,
    pub generation: GenerationId,
    pub code: CodeSource,
    pub loaded: Result<&'a crate::Loaded, &'a [Cause]>,
}
```

The generation that runs under one name in ordinary operation.

- `name` — Routing name.
- `generation` — The running generation.
- `code` — How its code entered the registry.
- `loaded` — What it declared when it loaded, or why this host cannot serve it.

## struct `ActiveConfig`

`src/registry.rs`

```rust
pub struct ActiveConfig {
    pub generation: GenerationId,
    pub document: serde_json::Value,
    pub kernel: crate::KernelSettings,
    /* private fields */
}
```

The active configuration generation: registered content plus the declarations derived from it.

The declarations are those the current host derives. They are private to the registry, so only
the registry builds one, from a single evaluation, and the parts cannot drift apart.

- `generation` — Active configuration generation.
- `document` — Registered document; never read from the intent file during use.
- `kernel` — Parsed kernel-owned tables.

Implements:

```rust
impl Clone for ActiveConfig
```

## enum `Answer`

`src/ports/decision.rs`

```rust
pub enum Answer {
    Predicate(f64),
    Choice(Vec<f64>),
    Score(Vec<f64>),
    Refused,
}
```

Finite probabilities in [0, 1], or a refusal for this question.
Choice and score distributions follow label order and sum to one, allowing rounding.

- `Predicate` — Probability that the predicate is true.
- `Choice` — Probability of each alternative, in label order.
- `Score` — Probability of each level, in label order.
- `Refused` — The service declined to answer this question.

Implements:

```rust
impl Debug for Answer
```

## enum `ArtifactSource`

`src/registry/request.rs`

```rust
pub enum ArtifactSource {
    Bytes(Vec<u8>),
    Stored(ContentHash),
}
```

Newly built bytes or a content address already known to the store.

- `Bytes` — Fresh component bytes, persisted only after acceptance.
- `Stored` — A retained artifact, or unchanged intent whose bytes need not be loaded.

### `ArtifactSource::hash`

```rust
pub fn hash(&self) -> ContentHash
```

Address the offered code without loading it or changing retention.

## enum `AttachmentContent`

`src/ports/channel.rs`

```rust
pub enum AttachmentContent {
    Image(Vec<u8>),
    Unavailable(String),
}
```

The result of reading an attachment.

- `Image` — Image bytes to validate and store.
- `Unavailable` — Why the protocol cannot provide a usable image.

Implements:

```rust
impl Debug for AttachmentContent
```

## struct `BoundComposer`

`src/registry/exports.rs`

```rust
pub struct BoundComposer {
    pub export: Export<dyn Composer>,
    pub options: crate::ComposerOptions,
}
```

A composer generation paired with options accepted by that generation's schema.

- `export` — Exact code selected for this composition.
- `options` — Normalized invocation options.

## struct `Budget`

`src/ports/composer.rs`

```rust
pub struct Budget {
    pub context_tokens: std::num::NonZeroU32,
    pub max_output_tokens: std::num::NonZeroU32,
}
```

Context limits supplied by configuration.

- `context_tokens` — Total model window including reserved output.
- `max_output_tokens` — Requested maximum generated tokens.

Implements:

```rust
impl Clone for Budget
impl Copy for Budget
impl Debug for Budget
```

## struct `CallContext`

`src/ports/tool.rs`

```rust
pub struct CallContext {
    pub session: SessionId,
    pub call: CallId,
    pub cancel: tokio_util::sync::CancellationToken,
    pub result_budget: usize,
}
```

Invocation identity, cooperative cancellation and the inline result budget.

- `session` — Session which proposed the call.
- `call` — Host-assigned identity used for durable settlement.
- `cancel` — Child of the Run cancellation token.
- `result_budget` — Maximum inline result size in bytes, including any continuation instructions.

## enum `Cause`

`src/registry/cause.rs`

```rust
pub enum Cause {
    Load {
        plugin: PluginName,
        candidate: Proposed,
        error: crate::LoadError,
    },
    Probe {
        plugin: PluginName,
        candidate: Proposed,
        failure: Failure,
    },
    NotLoaded {
        plugin: PluginName,
    },
    Environment {
        plugin: PluginName,
        failure: Failure,
    },
    ToolConflict {
        plugin: PluginName,
        tool: String,
        with: ToolOwner,
    },
    UnknownPlugin {
        user: String,
        target: UseRef,
        plugin: PluginName,
    },
    PluginUnavailable {
        user: String,
        target: UseRef,
        plugin: PluginName,
    },
    Use {
        user: String,
        target: UseRef,
        failure: Failure,
    },
    Table(crate::SettingsError),
    ProfileInUse {
        profile: String,
        sessions: Vec<String>,
    },
    SettingsWithoutPlugin {
        plugin: PluginName,
    },
    SettingsWithoutCode {
        plugin: PluginName,
    },
    NameReserved {
        name: PluginName,
    },
    NameTaken {
        name: PluginName,
    },
    Referenced {
        name: PluginName,
        by: Vec<Referrer>,
    },
}
```

A failure attributed to the part of the intent it concerns.

- `Load` — The candidate could not be loaded with its settings.
  - `plugin` — Plugin whose candidate failed.
  - `candidate` — What the candidate changes.
  - `error` — Failure reported while loading.
- `Probe` — The loaded candidate failed its local self-check.
  - `plugin` — Plugin whose candidate failed.
  - `candidate` — What the candidate changes.
  - `failure` — Failure returned by the probe.
- `NotLoaded` — The selected generation is not loaded on this host.
  - `plugin` — Plugin without loaded code.
- `Environment` — The component loads but lacks a requirement from its environment.
  - `plugin` — Plugin missing the requirement.
  - `failure` — Failure reported by the component.
- `ToolConflict` — An exported tool name is already taken.
  - `plugin` — Plugin exporting the tool.
  - `tool` — Conflicting tool name.
  - `with` — The host or the other plugins exporting it.
- `UnknownPlugin` — A configured invocation names a plugin that does not run.
  - `user` — Configured consumer.
  - `target` — Configuration entry named by that consumer.
  - `plugin` — Plugin named by the invocation.
- `PluginUnavailable` — A configured invocation names a plugin that runs but cannot serve calls.
  - `user` — Configured consumer.
  - `target` — Configuration entry named by that consumer.
  - `plugin` — Plugin named by the invocation.
- `Use` — A configured invocation cannot be served.
  - `user` — Configured consumer.
  - `target` — Configuration entry named by that consumer.
  - `failure` — Interface check or real call failure.
- `Table` — A configuration table cannot be interpreted.
- `ProfileInUse` — Removing a profile would strand the Sessions that use it.
  - `profile` — Profile missing from the configuration.
  - `sessions` — Names of the Sessions using it.
- `SettingsWithoutPlugin` — Settings name a plugin that this registration does not have.
  - `plugin` — Name in the settings table.
- `SettingsWithoutCode` — Settings changed for a plugin with no running code to pair them with.
  - `plugin` — Plugin whose settings changed.
- `NameReserved` — The host reserves this name for its factory code.
  - `name` — Reserved name.
- `NameTaken` — Another plugin already has this name.
  - `name` — Taken name.
- `Referenced` — The configuration still refers to the name.
  - `name` — Referenced name.
  - `by` — What refers to it.

### `Cause::part`

```rust
pub fn part(&self) -> IntentPart
```

The part of the intent that must change, or that the owner must provide.

Implements:

```rust
impl Clone for Cause
impl Debug for Cause
impl Display for Cause
impl Error for Cause
impl Serialize for Cause
```

## enum `ChannelAction`

`src/ports/channel.rs`

```rust
pub enum ChannelAction<T = ChannelAttachment> {
    Message(ChannelContent<T>),
    Sessions,
    SelectSession(String),
    Cancel,
    Backtrack,
    Everyone(Option<bool>),
}
```

The meaning of an incoming message, independent of protocol command syntax.

- `Message` — Text and attachments for the conversation's Session.
- `Sessions` — List available Sessions.
- `SelectSession` — Select or create the named Session.
- `Cancel` — Stop the conversation's current work.
- `Backtrack` — Return to the message named by the input's reply target.
- `Everyone` — Open or close a group to members; absent queries its current setting.

## struct `ChannelActivity`

`src/ports/channel.rs`

```rust
pub struct ChannelActivity {
    pub elapsed_seconds: u32,
    pub tools: Vec<ChannelToolActivity>,
}
```

A Run's current progress, containing only public tool names and outcomes.

- `elapsed_seconds` — Whole seconds elapsed since the Run started.
- `tools` — The latest tools in their start order, oldest first.

Implements:

```rust
impl Clone for ChannelActivity
```

## struct `ChannelAttachment`

`src/ports/channel.rs`

```rust
pub struct ChannelAttachment {
    pub what: String,
    pub locator: String,
}
```

An image still held by the protocol.

- `what` — A description, such as a file name and media type.
- `locator` — The mapper's opaque locator for fetching it.

## enum `ChannelCallError`

`src/ports/channel.rs`

```rust
pub enum ChannelCallError {
    Protocol(Box<ChannelFault>),
    Store(crate::StoreError),
}
```

A mapper or protocol failure, distinct from failing to record its evidence.

- `Protocol` — The mapper or remote protocol rejected the operation.
- `Store` — Persistence failed; the operation must stop rather than treating this as a protocol refusal.

Implements:

```rust
impl Debug for ChannelCallError
impl Display for ChannelCallError
impl Error for ChannelCallError
impl From<ChannelFault> for ChannelCallError
impl From<StoreError> for ChannelCallError
```

## struct `ChannelCommand`

`src/ports/channel.rs`

```rust
pub struct ChannelCommand {
    pub kind: ChannelCommandKind,
    pub description: String,
    pub direct: bool,
    pub group: bool,
}
```

A command offered by the host in protocol menus.

- `kind` — Which host command this is.
- `description` — Menu explanation supplied by the host.
- `direct` — Applies in one-to-one conversations.
- `group` — Applies in group conversations.

## enum `ChannelCommandKind`

`src/ports/channel.rs`

```rust
pub enum ChannelCommandKind {
    Session,
    Cancel,
    Backtrack,
    Everyone,
}
```

The commands understood by the connection owner.

- `Session` — List or select Sessions.
- `Cancel` — Stop current work.
- `Backtrack` — Return to a previous message.
- `Everyone` — Change who can address the Agent in a group.

## struct `ChannelContent`

`src/ports/channel.rs`

```rust
pub struct ChannelContent<T> {
    pub text: String,
    pub images: Vec<T>,
    pub unviewable: Vec<enco_core::Unviewable>,
}
```

Text and attachments before or after fetching.

- `text` — The message's text.
- `images` — Images in protocol order, before or after attachment fetching.
- `unviewable` — Other attachments and why they cannot be viewed.

Implements:

```rust
impl<T> From<&str> for ChannelContent<T>
impl<T> From<String> for ChannelContent<T>
```

## struct `ChannelFault`

`src/ports/channel.rs`

```rust
pub struct ChannelFault {
    pub failure: enco_core::Failure,
    pub unknown: bool,
    pub fatal: bool,
    pub retry_after: Option<std::time::Duration>,
    pub origin: Option<enco_core::MappingOrigin>,
}
```

A protocol failure and the facts that determine retry and settlement.

- `failure` — The reported failure.
- `unknown` — Effects may have occurred, forbidding automatic resend.
- `fatal` — The connection is unusable, rather than only this destination.
- `retry_after` — Protocol-requested delay before another attempt.
- `origin` — Recorded response call when this failure arose while mapping a response.

Implements:

```rust
impl Debug for ChannelFault
impl Display for ChannelFault
impl Error for ChannelFault
impl From<ChannelFault> for ChannelCallError
```

## struct `ChannelIdentity`

`src/ports/channel.rs`

```rust
pub struct ChannelIdentity {
    pub protocol: String,
    pub account: String,
    pub facts: serde_json::Value,
}
```

Stable account identity declared by a mapper.

- `protocol` — A nonempty protocol name without colons, such as `telegram`.
- `account` — A nonempty, opaque account ID within that protocol; colons are allowed.
- `facts` — Protocol facts needed by subsequent mapper calls.

### `ChannelIdentity::key`

```rust
pub fn key(&self) -> String
```

The connection-state key, `<protocol>:<account>`.

Implements:

```rust
impl Clone for ChannelIdentity
```

## struct `ChannelInput`

`src/ports/channel.rs`

```rust
pub struct ChannelInput {
    pub conversation: String,
    pub message: String,
    pub sender: Option<String>,
    pub sender_name: Option<String>,
    pub direct: bool,
    pub addressed: bool,
    pub reply_to: Option<String>,
    pub respond: Option<String>,
    pub action: ChannelAction,
}
```

An incoming message, before host authorization or attachment fetching.

- `conversation` — The conversation where replies go.
- `message` — This message's ID within its conversation.
- `sender` — The sender's protocol ID, when reported.
- `sender_name` — The sender's display name, when reported.
- `direct` — Whether the conversation is one to one.
- `addressed` — Whether this message addresses the bot.
- `reply_to` — The protocol ID of the message it replies to, when present.
- `respond` — The opaque handle to answer after acceptance and receipt acknowledgment.
- `action` — The host action requested by this message.

## struct `ChannelMessage`

`src/ports/channel.rs`

```rust
pub struct ChannelMessage {
    pub key: String,
    pub input: ChannelInput,
    pub origin: enco_core::MappingOrigin,
}
```

One protocol message, its acceptance key and recorded origin.

- `key` — Protocol key accepted at most once, including for commands.
- `input` — What this message asks the host to consider.
- `origin` — The recorded mapper call and this message's position in its result.

## enum `ChannelReading`

`src/ports/channel.rs`

```rust
pub enum ChannelReading {
    Batch {
        messages: Vec<ChannelMessage>,
        protocol: Option<serde_json::Value>,
        accepted: Option<oneshot::Sender<()>>,
        acknowledged: oneshot::Receiver<()>,
    },
    Reset {
        failure: enco_core::Failure,
        accepted: oneshot::Sender<()>,
    },
}
```

One arrival's messages and protocol checkpoint, delivered in receive order.

- `Batch` — Accept the messages in order before committing the optional protocol checkpoint.
  - `messages` — Messages emitted by this arrival, each accepted at most once by its key.
  - `protocol` — The next committed protocol state; absent leaves it unchanged.
  - `accepted` — Complete after acceptance when request-based reading must wait before reading again.
  - `acknowledged` — Completes after all receipt acknowledgments finish; answers must wait for it.
- `Reset` — Clear a protocol checkpoint rejected by the mapper before reading from its initial state.
  - `failure` — Why the checkpoint was rejected.
  - `accepted` — Complete after the cleared checkpoint is committed.

## struct `ChannelSent`

`src/ports/channel.rs`

```rust
pub struct ChannelSent {
    pub message: Option<String>,
    pub origin: enco_core::MappingOrigin,
}
```

The confirmed result of sending one part.

- `message` — Protocol ID, absent when the protocol does not expose one.
- `origin` — The recorded call interpreting the send response.

## struct `ChannelSettings`

`src/ports/channel.rs`

```rust
pub struct ChannelSettings {
    pub base_url: String,
    pub secrets: std::collections::BTreeMap<String, String>,
}
```

The service and secret names chosen for one connection; values remain in transport.

- `base_url` — HTTP origin and optional base path used by this connection.
- `secrets` — Mapper credential names mapped to environment variable names.

Implements:

```rust
impl Clone for ChannelSettings
```

## struct `ChannelToolActivity`

`src/ports/channel.rs`

```rust
pub struct ChannelToolActivity {
    pub name: String,
    pub state: ChannelToolState,
}
```

One tool call visible in a Run's progress view.

- `name` — The tool's model-visible name, without its arguments.
- `state` — The call's current outcome.

Implements:

```rust
impl Clone for ChannelToolActivity
```

## enum `ChannelToolState`

`src/ports/channel.rs`

```rust
pub enum ChannelToolState {
    Running,
    Ok,
    Failed,
    Unknown,
}
```

A tool's current state in a progress view.

- `Running` — The call is still running.
- `Ok` — The call succeeded.
- `Failed` — The call failed.
- `Unknown` — The call may have taken effect but its outcome is unknown.

Implements:

```rust
impl Clone for ChannelToolState
impl Copy for ChannelToolState
```

## struct `CodeProvenance`

`src/registry/intent.rs`

```rust
pub struct CodeProvenance {
    pub generation: GenerationId,
    pub source: CodeSource,
}
```

Where a generation's code came from; settings-only generations follow their code generation.

- `generation` — Generation that supplied the code.
- `source` — How that code entered the registry.

Implements:

```rust
impl Clone for CodeProvenance
impl Copy for CodeProvenance
impl Eq for CodeProvenance
impl PartialEq for CodeProvenance
impl Serialize for CodeProvenance
```

## enum `CodeSource`

`src/registry/intent.rs`

```rust
pub enum CodeSource {
    Factory,
    Directory {
        registration: RegistrationId,
        source: ContentHash,
    },
}
```

How a generation's code entered the registry.

- `Factory` — Code shipped with this host, registered or not.
- `Directory` — A registered source directory.
  - `registration` — Registration that registered the directory.
  - `source` — The directory's content, as stored when it was registered.

Implements:

```rust
impl Clone for CodeSource
impl Copy for CodeSource
impl Eq for CodeSource
impl PartialEq for CodeSource
impl Serialize for CodeSource
```

## struct `Commit`

`src/ports/store.rs`

```rust
pub struct Commit {
    pub entries: Vec<Entry>,
    pub plan: Option<std::sync::Arc<PlanWrite>>,
}
```

An atomic append of Log facts and consumption of their corresponding Inbox rows.

- `entries` — Ordered facts to append.
- `plan` — A request plan accepted together with the Attempt that refers to it.

## struct `Completion`

`src/ports/provider.rs`

```rust
pub struct Completion {
    pub message: Message,
    pub usage: Usage,
    pub stop: StopReason,
}
```

Provider output with host-assigned call identities.

- `message` — Assistant message, including opaque provider extensions.
- `usage` — Token usage reported by the provider.
- `stop` — Stop condition reported by the provider.

Implements:

```rust
impl Clone for Completion
impl Debug for Completion
```

## struct `CompletionEndpoint`

`src/profile.rs`

```rust
pub struct CompletionEndpoint {
    pub endpoint: crate::Endpoint,
    pub budget: crate::Budget,
    pub accepts: std::collections::BTreeSet<enco_core::Modality>,
}
```

A profile's endpoint with the context limits required by completion.

- `endpoint` — Resolved service shared with all endpoint users.
- `budget` — Context window and reserved output of this model.
- `accepts` — Non-text inputs accepted by this model.

Implements:

```rust
impl Clone for CompletionEndpoint
```

## struct `ComposeInput`

`src/ports/composer.rs`

```rust
pub struct ComposeInput {
    pub round: RoundId,
    pub home: std::path::PathBuf,
    pub now: DateTime<Utc>,
    pub timezone: Tz,
    pub session: SessionRecord,
    pub transcript: Transcript<LocatedImage>,
    pub previous_plan: Option<ContextPlan>,
    pub instructions: Option<String>,
    pub memories: Vec<Memory>,
    pub omitted: Vec<Omission>,
    pub unavailable: Vec<(String, Failure)>,
    pub tools: Vec<(CapabilityId, ToolSpec)>,
    pub lifeline: Vec<CapabilityId>,
    pub safe_mode: Option<SafeMode>,
    pub reply: ReplyTarget,
    pub compaction: Option<Budget>,
    pub compactions_left: u32,
}
```

Fixed Round inputs, with the selected budgets and recorded recovery facts for this composition.

- `round` — Identity used to seed this Round's deterministic runtime.
- `home` — Absolute home location used to describe paths to the Agent.
- `now` — UTC instant sampled once for this Round.
- `timezone` — Owner's time zone, shared with recurring schedules.
- `session` — Session metadata sampled for this Round.
- `transcript` — Canonical history after the latest compaction.
- `previous_plan` — Most recent reply plan; the composer decides whether to reuse it.
- `instructions` — The text of the owner's standing instructions; absent when there are none.
- `memories` — Pinned memories first, then those recalled for the latest input, most relevant first.
- `omitted` — Content the host could not supply, recorded with the plan whatever the composer returns.
- `unavailable` — Published plugins excluded from the Round's capabilities and their current reasons.
- `tools` — All capabilities which may be disclosed this Round.
- `lifeline` — Capabilities that every reply plan of this Session must disclose.
- `safe_mode` — Why this Round uses factory instructions and lifeline tools, if applicable.
- `reply` — The reply model this composition is for.
- `compaction` — The summarizing model's limits; absent when no compaction candidate is usable.
- `compactions_left` — Compactions this Round still allows; at zero the composer must return a plan or fail.

Implements:

```rust
impl Clone for ComposeInput
```

## struct `ComposerSettings`

`src/profile.rs`

```rust
pub struct ComposerSettings {
    pub plugin: enco_core::PluginName,
    pub options: ComposerOptions,
}
```

A profile's selected composer and its invocation options.

- `plugin` — Registered name of the plugin exporting composition.
- `options` — Options checked against the generation selected for each composition.

Implements:

```rust
impl Clone for ComposerSettings
impl Debug for ComposerSettings
impl<'de> Deserialize<'de> for ComposerSettings
impl PartialEq for ComposerSettings
```

## enum `Composition`

`src/ports/composer.rs`

```rust
pub enum Composition {
    Plan(CompositionPlan),
    Compact {
        upto: LogPos,
        plan: CompositionPlan,
    },
}
```

A ready reply request or a request to summarize earlier complete Rounds first.

- `Plan` — Ready for a reply attempt.
- `Compact` — Summarize an inclusive prefix ending at a complete Round.
  - `upto` — Inclusive compaction boundary.
  - `plan` — Tool-free request to generate the summary.

## enum `CompositionItem`

`src/ports/composer.rs`

```rust
pub enum CompositionItem {
    Message {
        role: Role,
        text: String,
        sources: Vec<Source>,
    },
    Log {
        pos: LogPos,
        images: bool,
    },
}
```

Text written by a composer, or a reference to a recorded message.

- `Message` — Inline text and the sources it renders.
  - `role` — Speaker of the text.
  - `text` — Text written by this composition.
  - `sources` — Sources used to write it, in rendering order.
- `Log` — An immutable message from the transcript.
  - `pos` — Position of the message.
  - `images` — Whether its images accompany the text.

Implements:

```rust
impl Clone for CompositionItem
impl Debug for CompositionItem
```

## struct `CompositionPlan`

`src/ports/composer.rs`

```rust
pub struct CompositionPlan {
    pub items: Vec<CompositionItem>,
    pub tools: Vec<CapabilityId>,
    pub max_output_tokens: Option<u32>,
    pub omitted: Vec<Omission>,
}
```

Composer output before the kernel freezes the Round's tool definitions with it.

- `items` — Messages in request order.
- `tools` — Capabilities selected from this Round.
- `max_output_tokens` — Requested output budget; absent means provider default.
- `omitted` — Content this policy left out, with its reasons.

Implements:

```rust
impl Clone for CompositionPlan
impl Debug for CompositionPlan
```

## struct `ConfigStatus`

`src/registry/status.rs`

```rust
pub struct ConfigStatus {
    pub running: ConfigurationRef,
    pub registered: ConfigurationRef,
    pub invalid: Vec<crate::SettingsError>,
    pub unavailable_uses: Vec<UnservedUse>,
}
```

Configuration is one intent document; a rollback changes only its active reference.

- `running` — Running configuration.
- `registered` — Configuration selected by the latest registration.
- `invalid` — Host tables which the current host cannot interpret.
- `unavailable_uses` — Uses which cannot currently be served.

## enum `ConfigTable`

`src/registry.rs`

```rust
pub enum ConfigTable {
    Single(&'static str),
    Named(&'static str),
}
```

The structural unit owned by one configuration declaration.

- `Single` — The table is one configuration section.
- `Named` — Each named entry is a configuration section.

### `ConfigTable::name`

```rust
pub fn name(self) -> &'static str
```

Top-level key owned by the declaration.

Implements:

```rust
impl Clone for ConfigTable
impl Copy for ConfigTable
```

## struct `ConfigurationSource`

`src/registry/request.rs`

```rust
pub struct ConfigurationSource {
    pub document: serde_json::Value,
    pub source: ContentHash,
}
```

Parsed configuration paired with the exact input retained by the host.

- `document` — Complete configuration content for validation.
- `source` — Content address of the original configuration file.

## struct `ConnectionWrite`

`src/ports/store.rs`

```rust
pub struct ConnectionWrite {
    pub key: String,
    pub state: serde_json::Value,
    pub update: Option<String>,
    pub session: Option<NewSession>,
    pub settlement: Option<DeliverySettlement>,
}
```

State returned to the connection owner for atomic acceptance.

- `key` — The connection-state key, `<protocol>:<account>`.
- `state` — Complete state snapshot owned by this connection.
- `update` — Protocol update accepted with this state, absent for outbound or cursor-only progress.
- `session` — The Session this update creates, such as a group's first, never adopting an existing one.
- `settlement` — Final settlement of a Log-derived delivery, if any.

Implements:

```rust
impl Clone for ConnectionWrite
```

## struct `ConsumedMessage`

`src/ports/store.rs`

```rust
pub struct ConsumedMessage {
    pub session: SessionId,
    pub pos: LogPos,
    pub owner: bool,
}
```

Where a channel message became a Session's input.

- `session` — The Session that consumed it.
- `pos` — Its position in that Session's Log.
- `owner` — Whether its sender was the owner when it was accepted.

## struct `ContextError`

`src/ports/context.rs`

```rust
pub struct ContextError(pub String);
```

A context source failed to supply its authoritative content.

Implements:

```rust
impl Debug for ContextError
impl Display for ContextError
impl Error for ContextError
```

## struct `ContextQuery`

`src/ports/context.rs`

```rust
pub struct ContextQuery {
    pub session: SessionRecord,
    pub latest_event: Option<Event>,
    pub new_input: bool,
    pub cancel: tokio_util::sync::CancellationToken,
}
```

Shared input to a Round's context sources.

- `session` — Session for which content is being assembled.
- `latest_event` — Latest consumed input; does not imply source data is unchanged.
- `new_input` — Whether this Round consumed any new Inbox input.
- `cancel` — Child of the current Run cancellation token.

## struct `Credential`

`src/ports/runtime.rs`

```rust
pub struct Credential {
    pub name: String,
    pub required: bool,
    pub placements: Vec<Placement>,
}
```

A declared credential; transport validates placement and alone reads its value.

- `name` — Name used in the owner's secret mapping.
- `required` — Whether the mapping must be present.
- `placements` — Every origin and location populated by transport; never empty.

### `Credential::caller_selected`

```rust
pub fn caller_selected(&self) -> bool
```

Whether one placement follows the caller's service, which also supplies this credential's secret.

Implements:

```rust
impl Clone for Credential
impl Serialize for Credential
```

## struct `DeliveryStatus`

`src/kernel.rs`

```rust
pub struct DeliveryStatus {
    pub settlement: DeliverySettlement,
    pub session: SessionRef,
}
```

A delivery failure with the Session name resolved before presentation.

- `settlement` — Original delivery and settlement facts.
- `session` — The Session that produced the delivery.

Implements:

```rust
impl Serialize for DeliveryStatus
```

## enum `Difference`

`src/registry/status.rs`

```rust
pub enum Difference {
    Name {
        running: PluginName,
    },
    Code,
    Settings,
}
```

One layer in which a registered plugin runs otherwise than registered.

- `Name` — The plugin runs under a name from an earlier registration.
  - `running` — Routing name.
- `Code` — The running code differs from the registered code.
- `Settings` — The running settings differ from the registered settings.

Implements:

```rust
impl Clone for Difference
impl Debug for Difference
impl Eq for Difference
impl PartialEq for Difference
impl Serialize for Difference
```

## struct `Endpoint`

`src/settings.rs`

```rust
pub struct Endpoint {
    pub name: String,
    pub plugin: enco_core::PluginName,
    pub settings: enco_core::ProviderSettings,
}
```

A resolved model service, independent of its caller and code generation.

- `name` — Name of the configured model service.
- `plugin` — Registered plugin name whose exports serve this endpoint.
- `settings` — Durable parameters copied into AttemptStarted.

Implements:

```rust
impl Clone for Endpoint
```

## struct `EndpointFailure`

`src/ports/composer.rs`

```rust
pub struct EndpointFailure {
    pub endpoint: String,
    pub failure: Failure,
}
```

A candidate and the reason the kernel passed it over.

- `endpoint` — Configured name, for diagnostics rather than combination identity.
- `failure` — Current unavailability or the last recorded failure of this combination.

Implements:

```rust
impl Clone for EndpointFailure
```

## struct `EndpointSpec`

`src/settings.rs`

```rust
pub struct EndpointSpec {
    pub endpoint: Endpoint,
    pub window_tokens: Option<std::num::NonZeroU32>,
    pub max_output_tokens: Option<std::num::NonZeroU32>,
    pub dimensions: Option<std::num::NonZeroUsize>,
    pub accepts: std::collections::BTreeSet<enco_core::Modality>,
}
```

A named model service; each user checks the facts it needs.

- `endpoint` — Resolved name, plugin and invocation parameters.
- `window_tokens` — Model's context window, if known.
- `max_output_tokens` — Model's output limit, if known.
- `dimensions` — Embedding width, if applicable.
- `accepts` — Non-text input capabilities declared by the owner.

Implements:

```rust
impl Clone for EndpointSpec
```

## struct `Endpoints`

`src/profile.rs`

```rust
pub struct Endpoints {
    /* private fields */
}
```

An ordered, nonempty set of completion candidates accepted by configuration parsing.

### `Endpoints::first`

```rust
pub fn first(&self) -> &CompletionEndpoint
```

The configured first choice, irrespective of its current availability.

### `Endpoints::rest`

```rust
pub fn rest(&self) -> &[CompletionEndpoint]
```

The fallbacks after the first choice, in configured order.

### `Endpoints::iter`

```rust
pub fn iter(&self) -> impl Iterator<Item = &CompletionEndpoint>
```

Candidates in the owner's configured order.

Implements:

```rust
impl Clone for Endpoints
```

## struct `Export`

`src/registry/exports.rs`

```rust
pub struct Export<T: ?Sized> {
    pub plugin: PluginId,
    pub generation: GenerationId,
    pub adapter: std::sync::Arc<T>,
}
```

An invocation's immutable association between code and registry identity.

- `plugin` — Plugin owning the generation.
- `generation` — Exact generation used for this invocation.
- `adapter` — Compiled adapter retained until this invocation ends.

## struct `Exports`

`src/registry/exports.rs`

```rust
pub struct Exports {
    /* private fields */
}
```

Derived routing view. Holding it does not prevent the registry from publishing a new view.

### `Exports::plugin_of`

```rust
pub fn plugin_of(&self, generation: GenerationId) -> Option<PluginId>
```

Look up any recorded generation, including failed or unloaded history.

### `Exports::channel`

```rust
pub fn channel(&self, plugin: &PluginName) -> Result<Export<dyn Channel>, Failure>
```

Resolve the active mapper; connection settings were checked during registration.

### `Exports::completion`

```rust
pub fn completion(&self, endpoint: &Endpoint, safe_mode: bool) -> Result<Export<dyn Provider>, Failure>
```

Resolve a completion use, using the Round's fixed mode.

### `Exports::embedding`

```rust
pub fn embedding(&self, endpoint: &Endpoint) -> Result<Export<dyn Embedding>, Failure>
```

Resolve embedding at the start of one synchronization call.

### `Exports::decision`

```rust
pub fn decision(&self, endpoint: &Endpoint) -> Result<Export<dyn Decision>, Failure>
```

Resolve decision at the start of one policy evaluation.

### `Exports::composer`

```rust
pub fn composer(&self, settings: &ComposerSettings, safe_mode: bool) -> Result<BoundComposer, Failure>
```

Select the current composer and normalize its profile options in one binding step.

Safe mode always uses the factory generation of the host's composer with empty options.

### `Exports::active`

```rust
pub fn active(&self) -> impl Iterator<Item = Active<'_>>
```

The generation that runs under each name in ordinary operation, in name order.

## struct `FactoryPlugin`

`src/registry.rs`

```rust
pub struct FactoryPlugin {
    pub name: PluginName,
    pub id: PluginId,
    pub artifact: Vec<u8>,
}
```

A plugin supplied with the host binary.

- `name` — Host-reserved routing name.
- `id` — Stable host-assigned identity, independent of component self-description.
- `artifact` — Embedded component bytes.

Implements:

```rust
impl Clone for FactoryPlugin
```

## struct `FactoryStatus`

`src/registry/intent.rs`

```rust
pub struct FactoryStatus {
    pub name: PluginName,
    pub generation: GenerationId,
    pub artifact: ContentHash,
    pub follow: bool,
}
```

Current host-owned recovery code and whether registered intent needs to follow it.

- `name` — Name reserved by the host.
- `generation` — Recorded embedded code.
- `artifact` — Content address of the embedded code.
- `follow` — Registered intent follows factory code and differs from this host's implementation.

Implements:

```rust
impl Clone for FactoryStatus
impl Serialize for FactoryStatus
```

## struct `HeaderPlacement`

`src/ports/runtime.rs`

```rust
pub struct HeaderPlacement {
    pub name: String,
    pub prefix: String,
}
```

A request header carrying a credential.

- `name` — Header name overwritten by the transport.
- `prefix` — Literal text before the secret value.

Implements:

```rust
impl Clone for HeaderPlacement
impl Serialize for HeaderPlacement
```

## enum `ImageError`

`src/image.rs`

```rust
pub enum ImageError {
    Unsupported,
    TooLarge {
        size: u64,
    },
    Store(crate::StoreError),
}
```

Image ingestion either rejects its input or reports a storage failure.

- `Unsupported` — The bytes have no supported image signature.
- `TooLarge` — The original bytes exceed the ingestion limit.
  - `size` — Original byte length.
- `Store` — The content store could not accept the image.

Implements:

```rust
impl Debug for ImageError
impl Display for ImageError
impl Error for ImageError
impl From<StoreError> for ImageError
```

## struct `Inspection`

`src/inspect.rs`

```rust
pub struct Inspection {
    pub attempt: AttemptId,
    pub purpose: AttemptPurpose,
    pub composer: GenerationId,
    pub provider: GenerationId,
    pub settings: ProviderSettings,
    pub request: crate::ProviderRequest<Image>,
    pub plan: ContextPlan,
    pub result: Option<AttemptResult>,
}
```

An Attempt reconstructed exclusively from its recorded plan and Log prefix.

- `attempt` — Recorded Attempt identity.
- `purpose` — Reply or compaction.
- `composer` — Composer generation which produced the frozen plan.
- `provider` — Provider generation to which the recorded request was bound.
- `settings` — Recorded invocation parameters, excluding the credential itself.
- `request` — Exact canonical request at this Attempt's Log position.
- `plan` — Frozen plan, including each inline message's sources and all recorded omissions.
- `result` — Recorded settlement, absent while the Attempt is unsettled.

Implements:

```rust
impl Debug for Inspection
impl<'de> Deserialize<'de> for Inspection
impl Serialize for Inspection
```

## enum `IntentPart`

`src/registry/cause.rs`

```rust
pub enum IntentPart {
    Code {
        plugin: PluginName,
    },
    Settings {
        plugin: PluginName,
    },
    Name {
        name: PluginName,
    },
    References {
        name: PluginName,
    },
    Use {
        user: String,
        target: UseRef,
    },
    Table {
        table: String,
    },
    Factory {
        plugin: PluginName,
    },
    Environment {
        plugin: PluginName,
    },
    Plugin {
        plugin: PluginName,
    },
}
```

The part of the owner's intent that a cause points to.

- `Code` — The source of a plugin.
  - `plugin` — Plugin whose code is at fault.
- `Settings` — The `[plugin.<name>]` table of a plugin.
  - `plugin` — Plugin whose settings are at fault.
- `Name` — A requested plugin name.
  - `name` — The name that cannot be used.
- `References` — Configuration that still refers to a plugin name.
  - `name` — The referenced name.
- `Use` — A configured invocation as one caller needs it.
  - `user` — Configured consumer, such as `profile default.reply`.
  - `target` — Configuration entry named by that consumer.
- `Table` — A top-level configuration table, including a name for named entries.
  - `table` — Table such as `memory` or `endpoint.chat`.
- `Factory` — Code shipped with this host; registration cannot change it.
  - `plugin` — Plugin whose factory code is at fault.
- `Environment` — A requirement the owner provides outside the intent, such as a secret value.
  - `plugin` — Plugin missing the requirement.
- `Plugin` — A plugin that cannot run; its own status says why.
  - `plugin` — The plugin that cannot run, by its current name.

Implements:

```rust
impl Clone for IntentPart
impl Debug for IntentPart
impl Eq for IntentPart
impl PartialEq for IntentPart
impl Serialize for IntentPart
```

## struct `Kernel`

`src/kernel.rs`

```rust
pub struct Kernel {
    /* private fields */
}
```

Single-node execution owner. Sessions serialize their own Log writes.

### `Kernel::store_image`

`src/image.rs`

```rust
pub async fn store_image(&self, bytes: &[u8]) -> Result<Image, ImageError>
```

Store image bytes through the same ingestion boundary used by tool settlement.

### `Kernel::inspect`

`src/inspect.rs`

```rust
pub async fn inspect(&self, session: SessionId, attempt: Option<AttemptId>) -> Result<Inspection, KernelError>
```

Inspect a specified Attempt, or the latest one, without composing or invoking a model.

### `Kernel::submit`

`src/kernel/input.rs`

```rust
pub async fn submit(&self, session: SessionId, event_id: EventId, text: String, images: Vec<Image>) -> Result<Accepted, KernelError>
```

Persist input before waking its owner; duplicate IDs remain harmless.

### `Kernel::backtrack`

`src/kernel/input.rs`

```rust
pub async fn backtrack(&self, session: SessionId, event_id: EventId, to: LogPos) -> Result<Accepted, KernelError>
```

Take the conversation back to a recorded position, as the owner asked from the CLI.

Like `submit`, a repeated event ID is accepted once.

### `Kernel::accept`

`src/kernel/input.rs`

```rust
pub async fn accept(&self, events: &[Event], connection: Option<&ConnectionWrite>) -> Result<Vec<Accepted>, KernelError>
```

Accept inputs and connection progress before waking the affected Session owners.

A Session the connection creates with this acceptance starts once it is committed. A
backtrack must point at a recorded position other than another backtrack; a newly accepted
one signals the Session's current Run, which stops at its next boundary.

### `Kernel::start`

```rust
pub async fn start(deps: KernelDeps) -> Result<Self, KernelError>
```

Bind capabilities and start one actor for each existing Session.

### `Kernel::open_session`

```rust
pub async fn open_session(&self, name: &str) -> Result<SessionRecord, KernelError>
```

Create a Session if needed and ensure its owner is running.

### `Kernel::connection_update_seen`

```rust
pub async fn connection_update_seen(&self, key: &str, update: &str) -> Result<bool, KernelError>
```

Check protocol update acceptance before preparing its content or routes.

### `Kernel::connection`

```rust
pub async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, KernelError>
```

Read a connection's last accepted state.

### `Kernel::delivery_failures`

```rust
pub async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliveryStatus>, KernelError>
```

Read recent final failures, retaining their original Session and Log references.

### `Kernel::consumed_message`

```rust
pub async fn consumed_message(&self, protocol: &str, account: &str, conversation: &str, message: &str) -> Result<Option<ConsumedMessage>, KernelError>
```

Find where a channel message with this protocol ID became a Session's input.

Commands recorded as inputs, such as backtracks, never match.

### `Kernel::delivered_message`

```rust
pub async fn delivered_message(&self, key: &str, target: &str, message: &str) -> Result<Option<Delivery>, KernelError>
```

Find the connection's delivery to `target` that sent the protocol message `message`.

### `Kernel::subscribe`

```rust
pub fn subscribe(&self, session: SessionId) -> Result<broadcast::Receiver<Entry>, KernelError>
```

Subscribe to newly committed facts; Log remains the durable source.

### `Kernel::cancel`

```rust
pub fn cancel(&self, session: SessionId) -> Result<bool, KernelError>
```

Signal the current Run; its actor settles work before ending.

### `Kernel::set_safe_mode`

```rust
pub async fn set_safe_mode(&self, enabled: bool) -> Result<(), KernelError>
```

Change the policy observed at the next Round boundary.

### `Kernel::set_profile`

```rust
pub async fn set_profile(&self, session: SessionId, profile: &str) -> Result<(), KernelError>
```

Persist a profile choice; the current Round keeps its sampled policy.

### `Kernel::status`

```rust
pub async fn status(&self) -> Result<Status, KernelError>
```

Inspect live state without involving the model.

### `Kernel::sessions`

```rust
pub async fn sessions(&self) -> Result<Vec<SessionRecord>, KernelError>
```

Read durable Session metadata.

### `Kernel::log`

```rust
pub async fn log(&self, session: SessionId, after: Option<LogPos>) -> Result<Vec<Entry>, KernelError>
```

Read immutable facts in structural order.

### `Kernel::registry`

```rust
pub fn registry(&self) -> &Arc<Registry>
```

Plugin commands share the registry used by completion and embedding callers.

### `Kernel::schedules`

```rust
pub fn schedules(&self) -> Schedules
```

Commands to the Scheduler, shared by CLI and model tools.

### `Kernel::shutdown`

```rust
pub async fn shutdown(&self) -> Result<(), KernelError>
```

Signal cancellation and wait for every owner to stop.

### `Kernel::outgoing_file`

`src/outgoing.rs`

```rust
pub async fn outgoing_file(&self, file: &OutgoingFile) -> Result<FileData, KernelError>
```

Resolve the recorded attachment before a channel starts its delivery.

Implements:

```rust
impl Drop for Kernel
```

## struct `KernelDeps`

`src/kernel.rs`

```rust
pub struct KernelDeps {
    pub store: std::sync::Arc<dyn Store>,
    pub registry: std::sync::Arc<Registry>,
    pub home: std::path::PathBuf,
    pub instructions: std::sync::Arc<dyn ContextSource<Supply = Option<String>>>,
    pub memories: std::sync::Arc<dyn ContextSource<Supply = OfferedMemories>>,
    pub tools: Vec<std::sync::Arc<dyn Tool>>,
    pub clock: std::sync::Arc<dyn Clock>,
}
```

Ports assembled once by the application; no service locator is involved.

- `store` — Durable authority.
- `registry` — Sole owner of plugin generations and published exports.
- `home` — Absolute home location supplied to context policies.
- `instructions` — The owner's standing instructions, read once per Round.
- `memories` — The memories offered once per Round.
- `tools` — Host capabilities exposed through the ordinary Tool port.
- `clock` — Clock supplied by the host or tests.

## enum `KernelError`

`src/kernel.rs`

```rust
pub enum KernelError {
    Store(StoreError),
    Schedule(ScheduleError),
    Registry(RegistryError),
    Config(String),
    ShuttingDown,
    TaskFailed(String),
    SessionStopped(SessionId, String),
    UnknownAttempt(SessionId, String),
    InvalidPlan {
        attempt: AttemptId,
        reason: String,
    },
    UnknownSession(SessionId),
    UnknownProfile(String),
    Backtrack {
        session: SessionId,
        to: LogPos,
        reason: &'static str,
    },
}
```

Public command failure; Round failures are recorded in the Log instead.

- `Store` — A durable operation failed.
- `Schedule` — A schedule command failed.
- `Registry` — A registry operation failed.
- `Config` — Invalid composition-root input.
- `ShuttingDown` — New owners cannot be registered after shutdown starts.
- `TaskFailed` — An owner task exited without completing its normal shutdown path.
- `SessionStopped` — The actor stopped after an owner could not commit its state.
- `UnknownAttempt` — The requested Session has no matching recorded Attempt.
- `InvalidPlan` — A persisted plan cannot be decoded or resolved against its recorded history.
  - `attempt` — Attempt which references the invalid plan.
  - `reason` — Decoding or resolution failure.
- `UnknownSession` — No such Session is registered.
- `UnknownProfile` — The requested profile is absent from the node configuration.
- `Backtrack` — A backtrack must point at a position that this Session's conversation has recorded.
  - `session` — The Session the owner addressed.
  - `to` — The position the owner pointed at.
  - `reason` — Why the position cannot be a target.

Implements:

```rust
impl Debug for KernelError
impl Display for KernelError
impl Error for KernelError
impl From<RegistryError> for KernelError
impl From<ScheduleError> for KernelError
impl From<StoreError> for KernelError
```

## struct `KernelSettings`

`src/settings.rs`

```rust
pub struct KernelSettings {
    pub timezone: enco_core::Tz,
    pub max_rounds_per_run: std::num::NonZeroU32,
    pub profiles: std::collections::BTreeMap<String, crate::Profile>,
    /* private fields */
}
```

Parsed kernel tables, sampled by each consumer at its own boundary.

- `timezone` — Owner's civil timezone.
- `max_rounds_per_run` — Maximum number of Rounds in one Run.
- `profiles` — Named Session policies, including `default`.

### `KernelSettings::TABLES`

```rust
const TABLES: [crate::ConfigTable; 4] = _;
```

Table ownership used by the registry's declaration evaluation.

### `KernelSettings::from_document`

```rust
pub fn from_document(document: &Value, factory_composer: &PluginName) -> Result<Self, SettingsError>
```

Parse and validate only the kernel-owned tables.

A profile that names no composer uses `factory_composer` with empty options.

### `KernelSettings::endpoint`

```rust
pub fn endpoint(&self, name: &str, table: &str) -> Result<&EndpointSpec, SettingsError>
```

Resolve a model service and attribute lookup failures to its user's table.

### `KernelSettings::uses`

```rust
pub fn uses(&self) -> Vec<Use>
```

Project each profile's requirements from its parsed endpoints.

Implements:

```rust
impl Clone for KernelSettings
```

## struct `Label`

`src/ports/decision.rs`

```rust
pub struct Label {
    pub name: String,
    pub description: Option<String>,
}
```

An option or score level. Names are unique within a question.

- `name` — Option name or score-level name.
- `description` — Optional rubric explaining when the label applies.

Implements:

```rust
impl Clone for Label
impl Debug for Label
```

## enum `LoadError`

`src/ports/runtime.rs`

```rust
pub enum LoadError {
    Component(String),
    Settings(String),
    Plugin(enco_core::Failure),
}
```

Why a component cannot be loaded, attributed to its code or to the settings it was given.

- `Component` — This host cannot compile, link or instantiate the component, or its declarations are invalid.
- `Settings` — The settings cannot be used with this component.
- `Plugin` — The component reported a failure while describing itself or its tools.

Implements:

```rust
impl Clone for LoadError
impl Debug for LoadError
impl Display for LoadError
impl Error for LoadError
impl Serialize for LoadError
```

## struct `Loaded`

`src/ports/runtime.rs`

```rust
pub struct Loaded {
    pub summary: String,
    pub config_schema: serde_json::Value,
    pub credentials: Vec<Credential>,
    pub channel_origins: Vec<String>,
    pub unavailable: Option<enco_core::Failure>,
    pub lifecycle: std::sync::Arc<dyn Lifecycle>,
    pub composer: Option<LoadedComposer>,
    pub channel: Option<std::sync::Arc<dyn Channel>>,
    pub completion: Option<std::sync::Arc<dyn Provider>>,
    pub embedding: Option<std::sync::Arc<dyn Embedding>>,
    pub decision: Option<std::sync::Arc<dyn Decision>>,
    pub tools: Option<LoadedTools>,
}
```

Exports returned by loading a component. Each invocation owns its own runtime state.

- `summary` — Adapter-provided description, not a plugin identity.
- `config_schema` — Plugin-owned configuration documentation, retained with the loaded generation.
- `credentials` — Credential declarations used by transport, wiring and generated documentation.
- `channel_origins` — Additional network origins declared by the channel mapper, fixed with its generation.
- `unavailable` — A missing environment requirement; the code still loads and retains its health.
- `lifecycle` — Self-check required before a deployment is admitted.
- `composer` — Context policy and its profile options, fixed with the loaded generation.
- `channel` — Messaging protocol mapper when supplied by the component.
- `completion` — Completion interface when supplied by the component.
- `embedding` — Embedding interface when supplied by the component.
- `decision` — Decision interface when supplied by the component.
- `tools` — Declared tool definitions and their common invocation interface.

Implements:

```rust
impl Clone for Loaded
```

## struct `LoadedComposer`

`src/ports/runtime.rs`

```rust
pub struct LoadedComposer {
    pub schema: enco_core::InputSchema,
    pub adapter: std::sync::Arc<dyn Composer>,
}
```

A composer's option declaration and invocation, loaded from the same content.

- `schema` — Profile options accepted by this generation.
- `adapter` — Writes requests from the host's fixed facts.

Implements:

```rust
impl Clone for LoadedComposer
```

## struct `LoadedTools`

`src/ports/runtime.rs`

```rust
pub struct LoadedTools {
    pub specs: Vec<enco_core::ToolSpec>,
    pub adapter: std::sync::Arc<dyn Tools>,
}
```

Tool declarations are evaluated once with the generation's accepted configuration.

- `specs` — Model-facing definitions fixed with this generation.
- `adapter` — Executes a tool with the definition supplied by its caller.

Implements:

```rust
impl Clone for LoadedTools
```

## struct `LocatedImage`

`src/ports/composer.rs`

```rust
pub struct LocatedImage {
    pub image: Image,
    pub path: std::path::PathBuf,
}
```

An image reference with the node-local address supplied by its store.

- `image` — Immutable image identity and metadata.
- `path` — Local file holding the bytes.

Implements:

```rust
impl Borrow<Image> for LocatedImage
impl Clone for LocatedImage
impl Debug for LocatedImage
```

## enum `Location`

`src/ports/runtime.rs`

```rust
pub enum Location {
    Header(HeaderPlacement),
    Path(Vec<String>),
    Frame(String),
}
```

Where transport alone writes a credential value.

- `Header` — One header populated with a prefix and the secret.
- `Path` — Insert immediately after the matching request-path prefix.
- `Frame` — Replace null at this JSON pointer in an outbound socket frame.

Implements:

```rust
impl Clone for Location
impl Serialize for Location
```

## enum `Need`

`src/registry.rs`

```rust
pub enum Need {
    Completion {
        tools: bool,
        max_output_tokens: std::num::NonZeroU32,
        accepts: std::collections::BTreeSet<Modality>,
    },
    Embedding {
        dimensions: std::num::NonZeroUsize,
    },
    Decision,
}
```

A use's interface and behavior requirements, shared by wiring checks and real probes.

- `Completion` — A completion endpoint, optionally with tool definitions.
  - `tools` — Whether normal requests include tools.
  - `max_output_tokens` — Output budget that normal requests declare.
  - `accepts` — Non-text inputs used by these completion calls.
- `Embedding` — Embeddings of the configured width.
  - `dimensions` — Expected vector width.
- `Decision` — Typed judgments for host policies.

Implements:

```rust
impl Clone for Need
impl Eq for Need
impl PartialEq for Need
```

## struct `NewSession`

`src/ports/store.rs`

```rust
pub struct NewSession {
    pub id: SessionId,
    pub name: String,
    pub profile: String,
    pub created_at: DateTime<Utc>,
}
```

A Session created together with the input that first reaches it.

- `id` — Identity chosen by the creator, which also records it in its own state.
- `name` — Display name; an occupied name receives a numeric suffix.
- `profile` — Registered profile the Session starts with.
- `created_at` — Creation time for display.

Implements:

```rust
impl Clone for NewSession
```

## struct `NodeRecord`

`src/ports/store.rs`

```rust
pub struct NodeRecord {
    pub id: NodeId,
    pub safe_mode: bool,
}
```

Persistent node identity and recovery policy.

- `id` — Persistent node identity.
- `safe_mode` — Whether the next Round uses only the lifeline.

Implements:

```rust
impl Clone for NodeRecord
impl Debug for NodeRecord
```

## struct `OfferedMemories`

`src/ports/context.rs`

```rust
pub struct OfferedMemories {
    pub memories: Vec<Memory>,
    pub omitted: Vec<Omission>,
}
```

The memories offered for one Round, and what could not be offered.

- `memories` — Pinned memories first, then those recalled for the latest input, most relevant first.
- `omitted` — Degraded or skipped recall, with reasons the plan records.

Implements:

```rust
impl Clone for OfferedMemories
impl Debug for OfferedMemories
impl Default for OfferedMemories
```

## struct `Placement`

`src/ports/runtime.rs`

```rust
pub struct Placement {
    pub origin: Option<String>,
    pub location: Location,
}
```

One credential recipient and the location populated for that recipient.

- `origin` — Fixed origin, with optional subdomain wildcard, or the service selected by the caller.
- `location` — Request or frame position written by the transport.

Implements:

```rust
impl Clone for Placement
impl Serialize for Placement
```

## struct `PlanWrite`

`src/ports/store.rs`

```rust
pub struct PlanWrite {
    /* private fields */
}
```

An immutable request plan whose complete content address is computed once before committing.

### `PlanWrite::new`

```rust
pub fn new(plan: ContextPlan) -> Result<Self, StoreError>
```

Prepare a plan for atomic acceptance with its Attempt, without changing storage.

### `PlanWrite::hash`

```rust
pub fn hash(&self) -> ContentHash
```

The address of the complete plan, independent of its storage representation.

### `PlanWrite::plan`

```rust
pub fn plan(&self) -> &ContextPlan
```

The exact content accepted with the Attempt and shared with the request sender.

## enum `PluginChange`

`src/registry/request.rs`

```rust
pub enum PluginChange {
    Factory,
    Code {
        source: ContentHash,
        artifact: ArtifactSource,
    },
    Remove,
    Rename(PluginName),
}
```

One plugin's contribution to an atomic registration.

- `Factory` — Current embedded code for a factory plugin without a source directory.
- `Code` — A component built from source intent.
  - `source` — Source tree already persisted by the host.
  - `artifact` — Compiled component offered for validation.
- `Remove` — Remove the identity associated with the registered source name.
- `Rename` — Change a registered name without changing code or settings.

## enum `PluginChangeKind`

`src/registry/history.rs`

```rust
pub enum PluginChangeKind {
    Created,
    Renamed {
        from: PluginName,
    },
    Removed,
    Code {
        generation: Box<GenerationRecord>,
    },
    Factory {
        generation: Box<GenerationRecord>,
    },
    Settings {
        generation: Box<GenerationRecord>,
    },
}
```

Accepted changes to one plugin; each variant carries only its own facts.

- `Created` — A previously unnamed plugin acquired a name.
- `Renamed` — The accepted name changed while retaining the plugin.
  - `from` — Name before this registration.
- `Removed` — The accepted name was removed.
- `Code` — Source selected new code.
  - `generation` — Selected generation and its current outcome.
- `Factory` — The host's factory code was selected.
  - `generation` — Selected generation and its current outcome.
- `Settings` — New settings selected a generation using existing code.
  - `generation` — Selected generation and its current outcome.

### `PluginChangeKind::generation`

```rust
pub fn generation(&self) -> Option<&GenerationRecord>
```

Generation selected by this change, including a reused factory generation.

Implements:

```rust
impl Clone for PluginChangeKind
impl Serialize for PluginChangeKind
```

## struct `PluginStatus`

`src/registry/status.rs`

```rust
pub struct PluginStatus {
    pub id: PluginId,
    pub name: PluginName,
    pub running: Running,
    pub rollback_target: Option<PluginActivation>,
    pub summary: Option<String>,
    pub users: Vec<String>,
    pub standing: Standing,
}
```

What runs for one plugin, and how that differs from its registered intent.

- `id` — Immutable plugin identity.
- `name` — Its registered name, or the name it runs under after a registration removed it.
- `running` — What runs now.
- `rollback_target` — Retained healthy generation, before the embedded factory fallback.
- `summary` — Summary of the selected component.
- `users` — Configured users of the running name.
- `standing` — How what runs stands against the registered intent.

Implements:

```rust
impl Serialize for PluginStatus
```

## struct `Probe`

`src/registry.rs`

```rust
pub struct Probe {
    pub user: String,
    pub endpoint: String,
}
```

A successful real call made while applying configuration.

- `user` — User whose requirement triggered this call.
- `endpoint` — Endpoint that was checked.

Implements:

```rust
impl Serialize for Probe
```

## struct `Profile`

`src/profile.rs`

```rust
pub struct Profile {
    pub composer: ComposerSettings,
    pub reply: Endpoints,
    pub compaction: Endpoints,
    pub requires_lifeline: bool,
    pub tools: Option<std::collections::BTreeSet<String>>,
    pub owner_context: bool,
}
```

Session policy sampled once at the start of a Round.

- `composer` — Plugin and typed invocation options used to compose this Session's requests.
- `reply` — Ordered candidates for ordinary replies.
- `compaction` — Ordered candidates for summarizing history.
- `requires_lifeline` — Whether every reply plan must disclose all lifeline tools.
- `tools` — Allowed tool names; absent exposes all available tools, including future additions.
- `owner_context` — Whether composition receives the owner's standing instructions and memories.

Implements:

```rust
impl Clone for Profile
```

## enum `Proposed`

`src/registry/cause.rs`

```rust
pub enum Proposed {
    Code,
    Settings,
    Factory,
}
```

What a candidate generation changes, which decides the part its failure concerns.

- `Code` — Source or a supplied component.
- `Settings` — New settings for the running code.
- `Factory` — Code shipped with this host.

Implements:

```rust
impl Clone for Proposed
impl Copy for Proposed
impl Debug for Proposed
impl Eq for Proposed
impl PartialEq for Proposed
impl Serialize for Proposed
```

## struct `ProviderRequest`

`src/ports/provider.rs`

```rust
pub struct ProviderRequest<I = ImageData> {
    pub messages: Vec<Message<I>>,
    pub tools: Vec<DisclosedTool>,
    pub max_output_tokens: Option<u32>,
}
```

A canonical model request. Session requests resolve a frozen, recorded plan;
registry probes contain only fixed validation content.

- `messages` — Canonical messages in request order.
- `tools` — Frozen definitions disclosed to the model.
- `max_output_tokens` — Optional output limit.

Implements:

```rust
impl<I: Clone> Clone for ProviderRequest<I>
impl<I: Debug> Debug for ProviderRequest<I>
impl<'de, I> Deserialize<'de> for ProviderRequest<I> where I: Deserialize<'de>
impl<I: PartialEq> PartialEq for ProviderRequest<I>
impl<I> Serialize for ProviderRequest<I> where I: Serialize
```

## struct `Question`

`src/ports/decision.rs`

```rust
pub struct Question {
    pub instructions: String,
    pub kind: QuestionKind,
}
```

Instructions and the allowed answer shape for one independent judgment.

- `instructions` — The judgment to make against the shared state.
- `kind` — Allowed outcomes and their interpretation.

Implements:

```rust
impl Clone for Question
impl Debug for Question
```

## enum `QuestionKind`

`src/ports/decision.rs`

```rust
pub enum QuestionKind {
    Predicate,
    Choice(Vec<Label>),
    Score(Vec<Label>),
}
```

A predicate, unordered alternatives, or score levels ordered from low to high.

- `Predicate` — A yes/no judgment.
- `Choice` — Mutually exclusive alternatives.
- `Score` — Ordered levels, from low to high.

Implements:

```rust
impl Clone for QuestionKind
impl Debug for QuestionKind
```

## enum `Referrer`

`src/registry/cause.rs`

```rust
pub enum Referrer {
    User {
        user: String,
    },
    Settings,
}
```

What in the configuration refers to a plugin name.

- `User` — A configured caller naming the plugin.
  - `user` — The user, such as `profile default.reply`.
- `Settings` — The plugin's own `[plugin.<name>]` table.

Implements:

```rust
impl Clone for Referrer
impl Debug for Referrer
impl Eq for Referrer
impl PartialEq for Referrer
impl Serialize for Referrer
```

## struct `Registered`

`src/registry/request.rs`

```rust
pub struct Registered {
    pub registration: RegistrationRecord,
    pub changes: Vec<UnitChange>,
    pub generations: Vec<GenerationRecord>,
    pub configuration: Option<ConfigGeneration>,
    pub probes: Vec<Probe>,
    pub users: Vec<String>,
    pub warnings: Vec<String>,
}
```

The result of one accepted registration, with the evidence gathered before acceptance.

- `registration` — The accepted registration.
- `changes` — The same named, typed description returned by registration history.
- `generations` — Newly registered plugin content.
- `configuration` — Newly registered configuration, when present.
- `probes` — Calls which verified new endpoint uses.
- `users` — Configured users affected by this registration.
- `warnings` — Post-commit cleanup errors; the registration has already been accepted.

Implements:

```rust
impl Serialize for Registered
```

## struct `RegisteredIntent`

`src/registry/intent.rs`

```rust
pub struct RegisteredIntent {
    pub base: Option<RegistrationBase>,
    pub plugins: std::collections::BTreeMap<PluginName, RegisteredPlugin>,
}
```

Only the facts needed to compare and prepare new intent.

- `base` — Absent only while preparing the first registration.
- `plugins` — Accepted and host-reserved names with their immutable identities.

### `RegisteredIntent::registration`

```rust
pub fn registration(&self) -> Option<RegistrationId>
```

The accepted position, absent only during bootstrap preparation.

### `RegisteredIntent::configuration`

```rust
pub fn configuration(&self) -> Option<&ConfigGeneration>
```

The accepted configuration, absent only during bootstrap preparation.

### `RegisteredIntent::accepted`

```rust
pub fn accepted(&self) -> Result<&RegistrationBase, RegistryError>
```

Require an accepted view for inspection or restoration.

### `RegisteredIntent::rewind_scope`

```rust
pub fn rewind_scope(&self, target: &Self) -> RewindScope
```

Names are not rewound: each plugin keeps the directory of its latest name.

### `RegisteredIntent::plugin_by_id`

```rust
pub fn plugin_by_id(&self, id: PluginId) -> Option<(&PluginName, &RegisteredPlugin)>
```

The name and intent of a plugin identity.

Implements:

```rust
impl Clone for RegisteredIntent
```

## struct `RegisteredPlugin`

`src/registry/intent.rs`

```rust
pub struct RegisteredPlugin {
    pub id: PluginId,
    pub named_by: Option<RegistrationId>,
    pub code: Option<RegisteredSource>,
    pub activation: Activation,
    pub factory: Option<FactoryStatus>,
}
```

An accepted name, plus the code choice and current host implementation available to it.

- `id` — Immutable plugin identity.
- `named_by` — Registration that supplied this name; absent for a newly supplied factory name.
- `code` — Last accepted code intent; absent for a factory name that has never registered one.
- `activation` — Generation selected by the latest accepted activation, or this host's unregistered factory code.
- `factory` — Host-owned code and its follow decision, when the name is reserved.

### `RegisteredPlugin::source`

```rust
pub fn source(&self) -> Option<ContentHash>
```

Address of the accepted source directory, absent when following factory code.

Implements:

```rust
impl Clone for RegisteredPlugin
impl Serialize for RegisteredPlugin
```

## struct `RegisteredSource`

`src/registry/intent.rs`

```rust
pub struct RegisteredSource {
    pub intent: CodeIntent,
    pub registration: RegistrationId,
    pub generation: GenerationId,
    pub artifact: ContentHash,
}
```

An explicitly registered code choice, independent of mechanical recovery.

- `intent` — Directory or host factory code.
- `registration` — Accepted activation, including reuse of an existing factory generation.
- `generation` — Generation selected by that activation.
- `artifact` — Artifact used to compare newly built code.

### `RegisteredSource::source`

```rust
pub fn source(&self) -> Option<ContentHash>
```

Address of the selected source directory, absent for factory code.

### `RegisteredSource::matches_source_artifact`

```rust
pub fn matches_source_artifact(&self, artifact: ContentHash) -> bool
```

A source build matches only a registered directory choice, even when factory bytes coincide.

Implements:

```rust
impl Clone for RegisteredSource
impl Serialize for RegisteredSource
```

## struct `RegistrationBase`

`src/registry/intent.rs`

```rust
pub struct RegistrationBase {
    pub id: RegistrationId,
    pub configuration: ConfigGeneration,
}
```

An accepted history position always has a complete configuration.

- `id` — Latest registration included in this view.
- `configuration` — Configuration selected by the latest configuration registration in the view.

Implements:

```rust
impl Clone for RegistrationBase
```

## struct `RegistrationDetails`

`src/registry/history.rs`

```rust
pub struct RegistrationDetails<S = SessionRef> {
    pub id: RegistrationId,
    pub previous: Option<RegistrationId>,
    pub initiator: Initiator<S>,
    pub created_at: DateTime<Utc>,
    pub changes: Vec<UnitChange>,
}
```

A bounded description of one accepted registration, without unrelated history.

- `id` — Accepted position in the registration history.
- `previous` — Preceding position, absent for the first registration.
- `initiator` — Caller and their explanation.
- `created_at` — Observation time, used only for presentation.
- `changes` — Typed changes derived from the accepted rows.

Implements:

```rust
impl<S> Serialize for RegistrationDetails<S> where S: Serialize
```

## struct `RegistrationRequest`

`src/registry/request.rs`

```rust
pub struct RegistrationRequest {
    pub initiator: Initiator,
    pub base: Option<RegistrationId>,
    pub configuration: Option<ConfigurationSource>,
    pub plugins: std::collections::BTreeMap<PluginName, PluginChange>,
}
```

One intentional change. All supplied parts are accepted or rejected together.

- `initiator` — Caller responsible for this change.
- `base` — Last accepted registration used to prepare the request; absent only on first boot.
- `configuration` — A changed configuration document, or the initial document at startup.
- `plugins` — Code, removal and name changes keyed by their user-facing names.

## struct `RegistrationWrite`

`src/ports/store/registry.rs`

```rust
pub struct RegistrationWrite {
    pub registration: RegistrationRecord,
    pub generations: Vec<GenerationRecord>,
    pub configuration: Option<ConfigGeneration>,
    pub plugins: Vec<PluginRecord>,
}
```

The sole committer assigns identities before handing a complete write to the store.

- `registration` — Accepted change, including the previous name references.
- `generations` — Plugin generations created in this change.
- `configuration` — A configuration to create and activate, when its intent changed.
- `plugins` — Final runtime references for each affected identity.

Implements:

```rust
impl Clone for RegistrationWrite
```

## struct `Registry`

`src/registry.rs`

```rust
pub struct Registry {
    /* private fields */
}
```

Routing is published only after the durable mutation commits.

### `Registry::rollback`

`src/registry/activation.rs`

```rust
pub async fn rollback(&self) -> Result<Rollback, RegistryError>
```

Restore the still-effective parts of the newest accepted change.

### `Registry::lifeline`

`src/registry/configuration.rs`

```rust
pub fn lifeline(&self) -> &[String]
```

The host's repair set, also used when validating profile tool restrictions.

### `Registry::set_profile`

`src/registry/configuration.rs`

```rust
pub async fn set_profile(&self, session: SessionId, profile: &str) -> Result<(), RegistryError>
```

Profile assignments share the registry's committer with configuration changes.

### `Registry::report`

`src/registry/health.rs`

```rust
pub async fn report(&self, generation: GenerationId, verdict: Verdict, session: Option<SessionId>) -> Result<(), RegistryError>
```

Only the current trial's attributable failures change health; late reports are harmless.

### `Registry::history`

`src/registry/history.rs`

```rust
pub async fn history(&self, before: Option<RegistrationId>, limit: usize) -> Result<Vec<RegistrationDetails>, RegistryError>
```

Read accepted changes newest first, stopping before the supplied cursor.

### `Registry::registration`

`src/registry/history.rs`

```rust
pub async fn registration(&self, id: RegistrationId) -> Result<RegistrationDetails, RegistryError>
```

Read one accepted registration by identity.

### `Registry::registered_intent`

`src/registry/intent.rs`

```rust
pub async fn registered_intent(&self, at: Option<RegistrationId>) -> Result<RegisteredIntent, RegistryError>
```

Read accepted intent without assembling runtime status or generation history.

### `Registry::check_rename`

`src/registry/registration.rs`

```rust
pub async fn check_rename(&self, name: &PluginName, to: &PluginName) -> Result<(), RegistryError>
```

Check the same rename candidate before the host moves its source directory.

### `Registry::register`

`src/registry/registration.rs`

```rust
pub async fn register(&self, request: RegistrationRequest) -> Result<Registered, RegistryError>
```

Validate outside the committer, then accept every part against one current composition.

### `Registry::status`

`src/registry/status.rs`

```rust
pub async fn status(&self) -> Result<RegistryStatus, RegistryError>
```

Inspect facts, never infer which sequence of recovery actions produced them.

### `Registry::open`

```rust
pub async fn open<E, F>(deps: RegistryDeps, bootstrap: impl FnOnce(RegisteredIntent) -> F, registry_error: impl Fn(RegistryError) -> E) -> Result<Arc<Self>, E>
where
    F: std::future::Future<Output = Result<RegistrationRequest, E>>
```

Restore registered behavior; only an empty registry consumes initial intent.

### `Registry::view`

```rust
pub fn view(&self) -> Arc<View>
```

Read an immutable view without retaining a watch borrow.

### `Registry::subscribe`

```rust
pub fn subscribe(&self) -> watch::Receiver<Arc<View>>
```

Each component follows changes at its own ownership boundary.

## struct `RegistryDeps`

`src/registry.rs`

```rust
pub struct RegistryDeps {
    pub store: std::sync::Arc<dyn Store>,
    pub runtime: std::sync::Arc<dyn Runtime>,
    pub factory: Vec<FactoryPlugin>,
    pub composer: PluginName,
    pub owners: Vec<TableOwner>,
    pub host_tool_names: Vec<String>,
    pub lifeline: Vec<String>,
    pub clock: std::sync::Arc<dyn Clock>,
}
```

Durable registry dependencies assembled by the application.

- `store` — Sole persistence port for registry facts and component artifacts.
- `runtime` — Component compilation and export discovery.
- `factory` — Host-reserved identities and embedded bytes.
- `composer` — The factory plugin that composes for profiles naming no composer, and in safe mode.
- `owners` — Host-owned tables and their declaration functions.
- `host_tool_names` — Names from the native tool declarations assembled by the composition root.
- `lifeline` — Native repair tool names required by profiles that request the lifeline.
- `clock` — Observation time, never generation ordering.

## enum `RegistryError`

`src/registry.rs`

```rust
pub enum RegistryError {
    NoRegistrations,
    Intent(String),
    Store(crate::StoreError),
    FactoryIdentity(PluginName),
    UnknownPlugin(PluginName),
    Rejected(Vec<Cause>),
    EmptyRequest,
    NotFactory(PluginName),
    FactoryRemoval(PluginName),
    UnknownGeneration(GenerationId),
    UnknownRegistration {
        registration: RegistrationId,
        latest: RegistrationId,
    },
    UnknownProfile(String),
    Config(crate::SettingsError),
    Bootstrap(String),
    TableOwner(String),
    NoRollbackTarget,
    Conflict(String),
}
```

An explicit registry operation could not complete.

- `NoRegistrations` — No registration exists yet; only bootstrap preparation may use an empty baseline.
- `Intent` — Recorded facts cannot form a complete accepted view.
- `Store` — A durable operation failed.
- `FactoryIdentity` — A stored factory identity differs from the host's reserved identity.
- `UnknownPlugin` — No registered plugin matches this name.
- `Rejected` — The intent is rejected; each cause names the part it concerns.
- `EmptyRequest` — A registration request supplied no change.
- `NotFactory` — Factory intent was requested for a name the host does not reserve.
- `FactoryRemoval` — Removal was requested for a host-reserved name; factory intent replaces source instead.
- `UnknownGeneration` — The requested number is absent from both histories.
- `UnknownRegistration` — No accepted change has this structural position.
  - `registration` — Requested position.
  - `latest` — Latest accepted position.
- `UnknownProfile` — The active configuration does not define this profile.
- `Config` — Registered kernel settings cannot be interpreted by this host.
- `Bootstrap` — No initial document is available for the first configuration registration.
- `TableOwner` — The host composition assigns a table to more than one owner.
- `NoRollbackTarget` — No accepted change remains to roll back.
- `Conflict` — The active generation or target health changed while the target was prepared.

Implements:

```rust
impl Debug for RegistryError
impl Display for RegistryError
impl Error for RegistryError
impl From<RegistryError> for KernelError
impl From<StoreError> for RegistryError
```

## struct `RegistryState`

`src/ports/store/registry.rs`

```rust
pub struct RegistryState {
    pub registrations: std::collections::BTreeMap<RegistrationId, RegistrationRecord>,
    pub plugins: std::collections::BTreeMap<PluginId, PluginRecord>,
    pub generations: std::collections::BTreeMap<GenerationId, GenerationRecord>,
    pub config_generations: std::collections::BTreeMap<GenerationId, ConfigGeneration>,
    pub active_config: Option<GenerationId>,
}
```

Durable facts; compiled code and admission are projections of this history.

- `registrations` — Accepted changes in structural order.
- `plugins` — Every installation, including stopped and removed identities.
- `generations` — Immutable plugin content and its observed health.
- `config_generations` — Immutable configuration content.
- `active_config` — Current configuration, absent only before initial registration.

Implements:

```rust
impl Clone for RegistryState
impl Default for RegistryState
```

## struct `RegistryStatus`

`src/registry/status.rs`

```rust
pub struct RegistryStatus {
    pub registered: RegisteredIntent,
    pub plugins: Vec<PluginStatus>,
    pub config: ConfigStatus,
    pub recent_registrations: Vec<RegistrationDetails>,
}
```

Plugin and configuration facts sampled under the same committer.

- `registered` — The same narrow accepted-intent query used to prepare registrations.
- `plugins` — Plugins that run or are registered.
- `config` — Current configuration and its validity.
- `recent_registrations` — Recent registrations from the same locked observation as the current references.

## struct `ReplyTarget`

`src/ports/composer.rs`

```rust
pub struct ReplyTarget {
    pub budget: Budget,
    pub accepts: std::collections::BTreeSet<Modality>,
    pub skipped: Vec<EndpointFailure>,
}
```

The reply model the kernel selected, and why earlier candidates were passed over.

- `budget` — Limits of the selected model.
- `accepts` — Non-text inputs accepted by the selected reply endpoint.
- `skipped` — Candidates before it that are unavailable or exhausted in this Run, in configured order.

Implements:

```rust
impl Clone for ReplyTarget
```

## struct `RewindScope`

`src/registry/intent.rs`

```rust
pub struct RewindScope {
    /* private fields */
}
```

The paths a rewind between two accepted states can address, decided once for every caller.

### `RewindScope::changed`

```rust
pub fn changed(&self) -> Result<RewindSelection, ScopeError>
```

Every path whose registered value differs.

### `RewindScope::select`

```rust
pub fn select(&self, paths: &[IntentPath]) -> Result<RewindSelection, ScopeError>
```

Exactly the listed paths, each addressed by its name.

### `RewindScope::addressable`

```rust
pub fn addressable(&self, id: PluginId) -> Option<&PluginName>
```

The directory of an identity when an explicit path addresses exactly that identity.

### `RewindScope::collisions`

```rust
pub fn collisions(&self, ids: impl IntoIterator<Item = PluginId>) -> BTreeSet<PluginName>
```

Directories that several of these identities would be written to.

## struct `RewindSelection`

`src/registry/intent.rs`

```rust
pub struct RewindSelection {
    pub configuration: bool,
    pub plugins: std::collections::BTreeMap<PluginId, PluginName>,
}
```

The configuration file and plugin directories one rewind writes.

- `configuration` — Whether `config.toml` is written.
- `plugins` — Each selected identity and the directory it is written to.

Implements:

```rust
impl Default for RewindSelection
```

## struct `Rollback`

`src/registry/request.rs`

```rust
pub struct Rollback {
    pub registration: RegistrationId,
    pub plugins: Vec<PluginRecord>,
    pub configuration: Option<GenerationId>,
    pub warnings: Vec<String>,
}
```

A completed owner rollback, which may stop plugins when no recoverable code remains.

- `registration` — Registration whose still-effective parts were restored.
- `plugins` — Runtime references after recovery.
- `configuration` — Configuration activated by the rollback, when it changed.
- `warnings` — Post-commit cleanup errors; the recovered references are already active.

Implements:

```rust
impl Serialize for Rollback
```

## enum `Running`

`src/registry/status.rs`

```rust
pub enum Running {
    Stopped,
    Serving {
        name: PluginName,
        activation: Activation,
    },
    Unavailable {
        name: PluginName,
        activation: Activation,
        causes: Vec<Cause>,
    },
}
```

The running layer of one plugin.

- `Stopped` — No generation runs under a name.
- `Serving` — This host serves the selected generation.
  - `name` — Routing name.
  - `activation` — Selected generation and its provenance.
- `Unavailable` — A generation is selected, but this host cannot serve it.
  - `name` — Routing name.
  - `activation` — Selected generation and its provenance.
  - `causes` — Why this host cannot serve it.

### `Running::name`

```rust
pub fn name(&self) -> Option<&PluginName>
```

The routing name, absent when nothing runs.

### `Running::activation`

```rust
pub fn activation(&self) -> Option<&Activation>
```

The selected generation, whether or not this host can serve it.

Implements:

```rust
impl Serialize for Running
```

## enum `SafeMode`

`src/ports/composer.rs`

```rust
pub enum SafeMode {
    Owner,
    Failed(Failure),
}
```

The reason for the Round's fixed recovery mode.

- `Owner` — The owner explicitly enabled the switch; only disabling it permits normal operation.
- `Failed` — This Run failed against the same registered behavior.

Implements:

```rust
impl Clone for SafeMode
```

## enum `ScheduleError`

`src/scheduler.rs`

```rust
pub enum ScheduleError {
    EmptyMessage,
    UnknownSession(SessionId),
    InvalidCron(String),
    NoOccurrenceAfter(DateTime<FixedOffset>),
    TooFrequent,
    Restore {
        id: ScheduleId,
        source: Box<ScheduleError>,
    },
    Store(crate::StoreError),
    Stopped(Option<String>),
}
```

Schedule validation or persistence failure.

- `EmptyMessage` — A reminder must contain meaningful text.
- `UnknownSession` — There is no such destination Session.
- `InvalidCron` — Cron parsing or evaluation failed.
- `NoOccurrenceAfter` — The rule has no occurrence after the supplied instant.
- `TooFrequent` — A likely typo would wake the Agent too frequently.
- `Restore` — A persisted schedule could not be restored.
  - `id` — Schedule whose rule failed.
  - `source` — Parsing or occurrence failure.
- `Store` — The durable authority failed.
- `Stopped` — The owner has stopped, with its reported failure when one exists.

Implements:

```rust
impl Debug for ScheduleError
impl Display for ScheduleError
impl Error for ScheduleError
impl From<ScheduleError> for KernelError
impl From<StoreError> for ScheduleError
```

## struct `Scheduled`

`src/scheduler.rs`

```rust
pub struct Scheduled {
    pub schedule: Schedule,
    pub next_due: DateTime<Utc>,
    pub timezone: Tz,
}
```

An active schedule with its derived next occurrence, for tools and administration.

- `schedule` — Durable rule and last delivered occurrence.
- `next_due` — Next planned delivery in UTC; this value is not persisted.
- `timezone` — Effective time zone for this view, including the owner's zone when implicit.

### `Scheduled::into_json`

```rust
pub fn into_json(self) -> Value
```

Shared tool and CLI view, with local offsets fixed when producing the result.
The rule echoes the creation arguments; `timezone` is the effective zone.

Implements:

```rust
impl Clone for Scheduled
impl Debug for Scheduled
```

## struct `Schedules`

`src/scheduler.rs`

```rust
pub struct Schedules {
    /* private fields */
}
```

Commands to the sole owner of durable schedules.

### `Schedules::create`

```rust
pub async fn create(&self, session: SessionId, rule: ScheduleRule, message: String) -> Result<Scheduled, ScheduleError>
```

Persist a schedule addressed to an existing Session.

### `Schedules::list`

```rust
pub async fn list(&self, session: Option<SessionId>) -> Result<Vec<Scheduled>, ScheduleError>
```

List active schedules in due order, optionally limited to one Session.

### `Schedules::cancel`

```rust
pub async fn cancel(&self, id: ScheduleId) -> Result<bool, ScheduleError>
```

Stop future occurrences. Events already in the Inbox are unaffected.

Implements:

```rust
impl Clone for Schedules
```

## enum `ScopeError`

`src/registry/intent.rs`

```rust
pub enum ScopeError {
    Unknown(PluginName),
    Collision(PluginName),
}
```

A requested rewind that the scope cannot address.

- `Unknown` — Neither state has a plugin under the name.
- `Collision` — Two selected identities would be written to the same directory.

Implements:

```rust
impl Debug for ScopeError
```

## struct `SessionStatus`

`src/kernel.rs`

```rust
pub struct SessionStatus {
    pub session: SessionRecord,
    pub running: bool,
    pub stopped: Option<String>,
}
```

Live execution status, separate from durable Session facts.

- `session` — Durable Session metadata.
- `running` — Whether a Run is currently executing.
- `stopped` — Storage failure that stopped this actor, if any.

Implements:

```rust
impl Serialize for SessionStatus
```

## struct `SettingsError`

`src/settings.rs`

```rust
pub struct SettingsError {
    pub table: String,
    pub message: String,
}
```

A configuration error attributed to the table that owns it.

- `table` — Owning table, including a name for endpoint and profile entries.
- `message` — Invalid field or relationship.

### `SettingsError::new`

```rust
pub fn new(table: impl Into<String>, message: impl ToString) -> Self
```

Attribute a parsing or validation failure to its table.

Implements:

```rust
impl Clone for SettingsError
impl Debug for SettingsError
impl Display for SettingsError
impl Error for SettingsError
impl Serialize for SettingsError
```

## enum `Standing`

`src/registry/status.rs`

```rust
pub enum Standing {
    Registered {
        differences: Vec<Difference>,
    },
    Removed {
        by: RegistrationId,
    },
}
```

How the running layer of one plugin stands against its registered layer.

- `Registered` — No difference is listed when what runs agrees, or when nothing runs.
  - `differences` — The layers that differ.
- `Removed` — A registration removed the plugin, and mechanical recovery still runs it.
  - `by` — Registration that removed it.

Implements:

```rust
impl Clone for Standing
impl Debug for Standing
impl Eq for Standing
impl PartialEq for Standing
impl Serialize for Standing
```

## struct `Status`

`src/kernel.rs`

```rust
pub struct Status {
    pub timezone: Tz,
    pub node: NodeId,
    pub safe_mode: bool,
    pub profiles: Vec<String>,
    pub sessions: Vec<SessionStatus>,
    pub scheduler_stopped: Option<String>,
}
```

Current node and Session execution state.

- `timezone` — Owner's timezone from the selected configuration.
- `node` — Persistent node identity.
- `safe_mode` — Policy observed at the next Round boundary.
- `profiles` — Available profile names; endpoint credentials are never exposed.
- `sessions` — State of every registered Session actor.
- `scheduler_stopped` — Failure that stopped the Scheduler, if any.

Implements:

```rust
impl Serialize for Status
```

## enum `StoreError`

`src/ports/store.rs`

```rust
pub enum StoreError {
    EmptyCommit,
    Fenced {
        session: SessionId,
        binding: Binding,
        epoch: Epoch,
    },
    OutOfOrder {
        session: SessionId,
        expected: LogPos,
        got: LogPos,
    },
    Inbox(String),
    ScheduleNotActive(ScheduleId),
    UnknownSession(SessionId),
    Blob(ContentHash),
    Plan {
        hash: ContentHash,
        reason: String,
    },
    Artifact(ContentHash),
    UnknownGeneration(GenerationId),
    SchemaVersion {
        found: u32,
        expected: u32,
    },
    Backend(String),
}
```

A failed storage operation or commit precondition.

- `EmptyCommit` — A Commit must append at least one fact.
- `Fenced` — The caller does not own this Session generation.
  - `session` — Session being committed.
  - `binding` — Current execution owner.
  - `epoch` — Generation supplied by the writer.
- `OutOfOrder` — The append would leave a gap or overwrite a fact.
  - `session` — Session being committed.
  - `expected` — Required next Log position.
  - `got` — Position supplied by the writer.
- `Inbox` — Input consumption does not match accepted facts.
- `ScheduleNotActive` — The scheduler's expected state no longer matches the authority.
- `UnknownSession` — The target Session does not exist.
- `Blob` — Immutable bytes are missing or do not match their address.
- `Plan` — A plan or one of its storage dependencies is missing or fails content verification.
  - `hash` — Address of the complete plan being restored.
  - `reason` — The missing dependency or invalid stored content.
- `Artifact` — Component bytes are missing or do not match their address.
- `UnknownGeneration` — A referenced registry record does not exist.
- `SchemaVersion` — The authority uses another schema version; opening it never deletes or migrates data.
  - `found` — Version recorded in the database.
  - `expected` — Version understood by this binary.
- `Backend` — The storage engine or filesystem operation failed.

Implements:

```rust
impl Debug for StoreError
impl Display for StoreError
impl Error for StoreError
impl From<StoreError> for ChannelCallError
impl From<StoreError> for ImageError
impl From<StoreError> for KernelError
impl From<StoreError> for RegistryError
impl From<StoreError> for ScheduleError
```

## struct `TableOwner`

`src/registry.rs`

```rust
pub struct TableOwner {
    pub tables: &'static [ConfigTable],
    pub declare: fn(&serde_json::Value, &crate::KernelSettings) -> Result<Vec<Use>, crate::SettingsError>,
}
```

A host-owned group of tables and its pure declaration function.

- `tables` — Top-level tables exclusively owned by this component.
- `declare` — Parse the owned tables and project their configured invocations.

## enum `ToolOwner`

`src/registry/admission.rs`

```rust
pub enum ToolOwner {
    Host,
    Plugins(Vec<PluginName>),
}
```

Who else exports a conflicting tool name.

- `Host` — A tool provided by the host.
- `Plugins` — Other plugins exporting the same name.

Implements:

```rust
impl Clone for ToolOwner
impl Debug for ToolOwner
impl Display for ToolOwner
impl Eq for ToolOwner
impl PartialEq for ToolOwner
impl Serialize for ToolOwner
```

## struct `Transcript`

`src/ports/composer.rs`

```rust
pub struct Transcript<I = Image> {
    pub summary: Option<String>,
    pub items: Vec<TranscriptItem<I>>,
    pub round_ends: Vec<LogPos>,
    pub run_ends: Vec<(LogPos, RunEnd)>,
}
```

Model-facing projection of immutable Log facts.

- `summary` — Latest replacement for summarized history.
- `items` — Canonical messages still visible after that summary, in ascending Log position order.
- `round_ends` — Complete Round boundaries eligible for compaction, in ascending Log position order.
- `run_ends` — Run outcomes after the summary, in Log order; the composer decides how to describe them.

### `Transcript::map_images`

```rust
pub fn map_images<J>(self, image: impl FnMut(I) -> J) -> Transcript<J>
```

Change the image representation without copying the other message content.

Implements:

```rust
impl<I: Clone> Clone for Transcript<I>
impl<I: Debug> Debug for Transcript<I>
impl<I> Default for Transcript<I>
```

## struct `TranscriptItem`

`src/ports/composer.rs`

```rust
pub struct TranscriptItem<I = Image> {
    pub pos: LogPos,
    pub message: Message<I>,
    pub generation: Option<GenerationId>,
    pub event: Option<Event>,
}
```

One canonical message and its immutable source.

- `pos` — Position of the originating fact.
- `message` — Canonical message used when resolving a plan reference.
- `generation` — Provider generation of the producing Attempt, present only for Assistant messages.
- `event` — Consumed Inbox Event, present only for inputs; the composer narrates its circumstances.

Implements:

```rust
impl<I: Clone> Clone for TranscriptItem<I>
impl<I: Debug> Debug for TranscriptItem<I>
```

## enum `UnitChange`

`src/registry/history.rs`

```rust
pub enum UnitChange {
    Plugin {
        name: PluginName,
        change: PluginChangeKind,
    },
    Configuration {
        generation: GenerationId,
        sections: Vec<String>,
    },
}
```

One path's accepted change, named at the registration where it happened.

- `Plugin` — An accepted plugin change under its name at this registration.
  - `name` — Name at the time of the change.
  - `change` — What changed for this plugin.
- `Configuration` — Configuration sections changed under their declared ownership.
  - `generation` — Configuration generation selected by this registration.
  - `sections` — Changed sections at the granularity declared by their owners.

### `UnitChange::path`

```rust
pub fn path(&self) -> IntentPath
```

The public path used by history, status and rewind.

Implements:

```rust
impl Clone for UnitChange
impl Serialize for UnitChange
```

## struct `UnservedUse`

`src/registry/status.rs`

```rust
pub struct UnservedUse {
    pub user: String,
    pub target: UseRef,
    pub causes: Vec<Cause>,
}
```

One current configured use that cannot be served by the selected exports.

- `user` — Configured consumer, such as `memory.embedding`.
- `target` — Configuration entry named by that consumer.
- `causes` — Why the selected exports cannot serve it.

Implements:

```rust
impl Serialize for UnservedUse
```

## struct `Use`

`src/registry.rs`

```rust
pub struct Use {
    pub user: String,
    pub target: UseTarget,
}
```

One configured caller and the invocation it requires.

- `user` — Human-readable caller, such as `profile default.reply`.
- `target` — The invocation this caller configures.

Implements:

```rust
impl Clone for Use
```

## enum `UseRef`

`src/registry.rs`

```rust
pub enum UseRef {
    Channel {
        name: String,
    },
    Endpoint {
        endpoint: String,
    },
    Composer {
        profile: String,
    },
}
```

The configuration entry a use names, without copying its invocation parameters.

- `Channel` — A named channel connection.
  - `name` — Name in the channel table.
- `Endpoint` — A named model service.
  - `endpoint` — Name in the endpoint table.
- `Composer` — A profile's composer selection and options.
  - `profile` — Name in the profile table.

Implements:

```rust
impl Clone for UseRef
impl Debug for UseRef
impl Display for UseRef
impl Eq for UseRef
impl PartialEq for UseRef
impl Serialize for UseRef
```

## enum `UseTarget`

`src/registry.rs`

```rust
pub enum UseTarget {
    Channel {
        name: String,
        plugin: PluginName,
        settings: crate::ChannelSettings,
    },
    Endpoint {
        endpoint: crate::Endpoint,
        need: Need,
    },
    Composer {
        profile: String,
        settings: crate::ComposerSettings,
    },
}
```

The configured invocation, preserving the facts its actual interface consumes.

- `Channel` — One connection's pure channel mapper and credential bindings.
  - `name` — Connection name in the channel table.
  - `plugin` — Registered plugin exporting the protocol mapper.
  - `settings` — Transport parameters and credential names.
- `Endpoint` — A model service and the behavior its consumer requires.
  - `endpoint` — Named service and its invocation parameters.
  - `need` — Required model interface and behavior.
- `Composer` — A profile's pure composer invocation.
  - `profile` — Profile owning these options, used to attribute repair.
  - `settings` — Plugin and invocation options.

Implements:

```rust
impl Clone for UseTarget
```

## enum `Verdict`

`src/registry.rs`

```rust
pub enum Verdict {
    Ok,
    Failed(Failure),
}
```

One completed invocation's verdict, before caller-specific result handling.

- `Ok` — The result passed its interface contract checks, including composer plan binding.
- `Failed` — A failed invocation; only attributable plugin faults affect health.

## struct `View`

`src/registry.rs`

```rust
pub struct View {
    pub exports: Exports,
    pub config: ActiveConfig,
}
```

The one published snapshot of routing and registered configuration.

- `exports` — Derived invocation routes.
- `config` — Registered content and parsed kernel tables.

## trait `Channel`

`src/ports/channel.rs`

```rust
pub trait Channel: Send + Sync
```

Opens accounts through one loaded, pure protocol mapper.

### `Channel::open`

```rust
async fn open(&self, settings: &ChannelSettings, generation: GenerationId, store: Arc<dyn Store>) -> Result<Arc<dyn ChannelAdapter>, ChannelCallError>
```

Bind transport settings and identify the account, retaining this generation for all calls.

## trait `ChannelAdapter`

`src/ports/channel.rs`

```rust
pub trait ChannelAdapter: Send + Sync
```

One identified account, using the same generation until it is stopped.

### `ChannelAdapter::identity`

```rust
fn identity(&self) -> ChannelIdentity
```

The stable account reached by this connection.

### `ChannelAdapter::receive`

```rust
async fn receive(&self, protocol: Option<Value>, readings: mpsc::UnboundedSender<ChannelReading>, stop: CancellationToken) -> Result<(), ChannelCallError>
```

Receive arrivals without waiting for preparation; drain receipt acknowledgments on stop.

### `ChannelAdapter::fetch`

```rust
async fn fetch(&self, locator: &str) -> Result<AttachmentContent, ChannelCallError>
```

Fetch an authorized attachment; reads have no external effects.

### `ChannelAdapter::split`

```rust
async fn split(&self, message: Outgoing<FileData>) -> Result<Vec<Outgoing<FileData>>, ChannelCallError>
```

Preserve all text and attach a file once, propagating mapper invocation failures.

### `ChannelAdapter::send`

```rust
async fn send(&self, target: &str, message: &Outgoing<FileData>) -> Result<ChannelSent, ChannelCallError>
```

Send once without internal retries, retaining evidence for the returned outcome.

### `ChannelAdapter::commands`

```rust
async fn commands(&self, commands: &[ChannelCommand]) -> Result<(), ChannelCallError>
```

Offer the host's commands in the protocol's menus.

### `ChannelAdapter::respond`

```rust
async fn respond(&self, handle: &str, text: Option<&str>) -> Result<(), ChannelCallError>
```

Answer an accepted message after its receipt acknowledgment completes.

### `ChannelAdapter::activity`

```rust
async fn activity(&self, target: &str, shown: Option<&str>, activity: Option<&ChannelActivity>) -> Result<Option<ChannelSent>, ChannelCallError>
```

Create, refresh or remove an activity view; creation returns its recorded response, if any.

## trait `Clock`

`src/ports/clock.rs`

```rust
pub trait Clock: Send + Sync
```

Time boundary used for reminders and observation, never Log ordering.

### `Clock::now`

```rust
fn now(&self) -> DateTime<Utc>
```

Current UTC instant; local time is derived using the owner's configured zone.

## trait `Composer`

`src/ports/composer.rs`

```rust
pub trait Composer: Send + Sync
```

Pure policy boundary: identical inputs must produce identical plans.

### `Composer::compose`

```rust
async fn compose(&self, options: &crate::ComposerOptions, input: &ComposeInput) -> Result<Composition, Failure>
```

Select, order and budget content using options already checked against this generation.

## trait `ContextSource`

`src/ports/context.rs`

```rust
pub trait ContextSource: Send + Sync
```

Supplies one owner's facts for a Round, in that owner's type; never called in safe mode.

### `ContextSource::Supply`

```rust
type Supply: Send;
```

What this source supplies.

### `ContextSource::contribute`

```rust
async fn contribute(&self, query: &ContextQuery) -> Result<<Self as >::Supply, ContextError>
```

Stop external waits on cancellation, then finish any local write before returning.

## trait `Decision`

`src/ports/decision.rs`

```rust
pub trait Decision: Send + Sync
```

Typed judgments consumed by host policies, independently of Session completion.

### `Decision::decide`

```rust
async fn decide(&self, settings: &ProviderSettings, state: String, questions: Vec<Question>) -> Result<Vec<Answer>, Failure>
```

Evaluate each question against the same state; answers follow question order.

## trait `Embedding`

`src/ports/embedding.rs`

```rust
pub trait Embedding: Send + Sync
```

Embedding boundary consumed by host memory, independently of completion.

### `Embedding::embed`

```rust
async fn embed(&self, settings: &ProviderSettings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure>
```

Embed texts in input order; credentials are never persisted with settings.
Return one nonempty, finite vector per input, all with the same width.

## trait `Lifecycle`

`src/ports/runtime.rs`

```rust
pub trait Lifecycle: Send + Sync
```

Component lifecycle shared by all exported interfaces.

### `Lifecycle::probe`

```rust
async fn probe(&self) -> Result<(), Failure>
```

Run a local self-check without contacting external services.

## trait `Provider`

`src/ports/provider.rs`

```rust
pub trait Provider: Send + Sync
```

Completion boundary. Implementations own timeouts and per-invocation state.

### `Provider::complete`

```rust
async fn complete(&self, settings: &ProviderSettings, request: ProviderRequest) -> Result<Completion, Failure>
```

Complete a frozen request; the transport resolves credentials from the settings.
Dropping the future cancels local work.

## trait `Runtime`

`src/ports/runtime.rs`

```rust
pub trait Runtime: Send + Sync
```

Compile and describe component bytes without knowing registry identities or routing.

### `Runtime::load`

```rust
async fn load(&self, artifact: &[u8], settings: &PluginSettings) -> Result<Loaded, LoadError>
```

Load the component and validate its imports and configuration.

## trait `Store`

`src/ports/store.rs`

```rust
pub trait Store: Send + Sync
```

Persistence boundary; only the owning actor may commit its state.

### `Store::node`

```rust
async fn node(&self) -> Result<NodeRecord, StoreError>
```

Read persistent node identity and safe-mode state.

### `Store::set_safe_mode`

```rust
async fn set_safe_mode(&self, enabled: bool) -> Result<(), StoreError>
```

Kernel-owned update, observed at the next Round boundary.

### `Store::sessions`

```rust
async fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError>
```

List existing Sessions.

### `Store::session`

```rust
async fn session(&self, id: SessionId) -> Result<Option<SessionRecord>, StoreError>
```

Read the metadata sampled by the Session actor at each Round boundary.

### `Store::session_by_name`

```rust
async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError>
```

Look up a Session without creating it.

### `Store::ensure_session`

```rust
async fn ensure_session(&self, name: &str, created_at: DateTime<Utc>) -> Result<SessionRecord, StoreError>
```

Create if absent, with local binding at epoch one and the default profile.

### `Store::log`

```rust
async fn log(&self, session: SessionId, after: Option<LogPos>) -> Result<Vec<Entry>, StoreError>
```

Read immutable facts in structural order after an optional position.

### `Store::commit`

```rust
async fn commit(&self, session: SessionId, commit: Commit) -> Result<(), StoreError>
```

Atomically append facts and consume their Inbox inputs after checking ownership and positions.

### `Store::plan`

```rust
async fn plan(&self, hash: &ContentHash) -> Result<ContextPlan, StoreError>
```

Restore a recorded request plan and verify its complete content address.

### `Store::consumed_message`

```rust
async fn consumed_message(&self, protocol: &str, account: &str, conversation: &str, message: &str) -> Result<Option<ConsumedMessage>, StoreError>
```

Find where a channel message with this protocol ID was consumed.

Commands recorded as inputs, such as backtracks, never match.

### `Store::accept`

```rust
async fn accept(&self, events: &[Event], connection: Option<&ConnectionWrite>) -> Result<Acceptance, StoreError>
```

Atomically accept Events with a connection's state, and with it the protocol update, the
Session that update creates and a delivery outcome, if any.

Accepting an update key twice is an error: the connection checks it before preparing.

### `Store::connection_update_seen`

```rust
async fn connection_update_seen(&self, key: &str, update: &str) -> Result<bool, StoreError>
```

Check durable update acceptance before preparing attachments or creating Sessions.

### `Store::connection`

```rust
async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, StoreError>
```

Read the state owned by one channel connection.

### `Store::delivery_failures`

```rust
async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliverySettlement>, StoreError>
```

Read recent failed or uncertain deliveries in reverse commit order.

### `Store::delivered_message`

```rust
async fn delivered_message(&self, key: &str, target: &str, message: &str) -> Result<Option<Delivery>, StoreError>
```

Find the connection's delivery to `target` whose confirmed parts include the protocol
message `message`.

### `Store::pending`

```rust
async fn pending(&self, session: SessionId) -> Result<Vec<Event>, StoreError>
```

Read unconsumed inputs in acceptance order.

### `Store::insert_schedule`

```rust
async fn insert_schedule(&self, schedule: &Schedule) -> Result<(), StoreError>
```

Scheduler-owned insertion of a new schedule, stored as given.

### `Store::cancel_schedule`

```rust
async fn cancel_schedule(&self, id: ScheduleId) -> Result<bool, StoreError>
```

Scheduler-owned cancellation; return whether an active schedule changed.

### `Store::schedules`

```rust
async fn schedules(&self, state: Option<ScheduleState>) -> Result<Vec<Schedule>, StoreError>
```

Read schedules, optionally filtered by state.

### `Store::fire_schedule`

```rust
async fn fire_schedule(&self, expected_last: Option<DateTime<Utc>>, done: bool, event: &Event) -> Result<(), StoreError>
```

Advance an active Schedule from expected_last and accept its Event atomically.
Mark it done when no occurrence remains.

### `Store::set_profile`

```rust
async fn set_profile(&self, id: SessionId, profile: &str) -> Result<(), StoreError>
```

Select the profile under the registry committer, effective at the next Round.

### `Store::registry`

```rust
async fn registry(&self) -> Result<RegistryState, StoreError>
```

Read registration history and the references restored at startup.

### `Store::record_factories`

```rust
async fn record_factories(&self, generations: &[GenerationRecord]) -> Result<(), StoreError>
```

Record embedded factory content without activating it or creating a registration.

### `Store::register`

```rust
async fn register(&self, write: &RegistrationWrite) -> Result<(), StoreError>
```

Atomically accept one registration, its content and its runtime references.

### `Store::promote`

```rust
async fn promote(&self, generation: GenerationId) -> Result<(), StoreError>
```

Promote an observed trial without changing its identity or rollback target.

### `Store::activate`

```rust
async fn activate(&self, change: &ActivationWrite) -> Result<(), StoreError>
```

Atomically move runtime references and record attributable failures and a notice.

### `Store::retain_artifacts`

```rust
async fn retain_artifacts(&self, hashes: &std::collections::HashSet<ContentHash>) -> Result<(), StoreError>
```

Remove component files which no runtime reference retains.

### `Store::put_artifact`

```rust
async fn put_artifact(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>
```

Persist immutable component bytes separately from removable history blobs.

### `Store::artifact`

```rust
async fn artifact(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>
```

Read and verify component bytes by content address.

### `Store::put_blob`

```rust
async fn put_blob(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>
```

Persist immutable bytes before recording their content address.

### `Store::get_blob`

```rust
async fn get_blob(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>
```

Read bytes and verify they match their recorded address.

### `Store::blob_path`

```rust
fn blob_path(&self, hash: &ContentHash) -> PathBuf
```

Local path of a blob, recorded with the truncation fact of a long tool result.

## trait `Tool`

`src/ports/tool.rs`

```rust
pub trait Tool: Send + Sync
```

An executable capability; dispatch is independent of tool identity.

### `Tool::spec`

```rust
fn spec(&self) -> ToolSpec
```

Model-facing contract, frozen into the Round's plan.

### `Tool::code`

```rust
fn code(&self) -> CodeRef
```

Code implementing the invocation.

### `Tool::call`

```rust
async fn call(&self, ctx: CallContext, args: serde_json::Map<String, serde_json::Value>) -> Outcome
```

Execute a call whose disclosure and arguments dispatch has already checked.

The arguments fit the tool's schema, with optional properties given as null removed.
Respond to cancellation and wait until started work is quiescent before returning.
Pageable results should fit `ctx.result_budget` and explain how to continue.

## trait `Tools`

`src/ports/runtime.rs`

```rust
pub trait Tools: Send + Sync
```

A plugin's tools interface, independent of the registry-assigned code identity.

### `Tools::call`

```rust
async fn call(&self, ctx: CallContext, spec: &ToolSpec, args: serde_json::Map<String, serde_json::Value>) -> Outcome
```

Execute a declared tool and wait for any started work to become quiescent on cancellation.

## fn `builtin_tool_specs`

`src/builtin.rs`

```rust
pub fn builtin_tool_specs() -> Result<Vec<ToolSpec>, SchemaError>
```

Native declarations available before their stateful handlers are constructed.

## fn `parse_table`

`src/settings.rs`

```rust
pub fn parse_table<T: DeserializeOwned>(document: &serde_json::Value, table: &str) -> Result<T, SettingsError>
```

Parse one owner's table without interpreting other tables.

## fn `plugin_settings`

`src/registry/configuration.rs`

```rust
pub fn plugin_settings(document: &serde_json::Value) -> Result<std::collections::BTreeMap<PluginName, PluginSettings>, crate::SettingsError>
```

Parse plugin settings using the same ownership boundary as registration.

## type `ComposerOptions`

`src/profile.rs`

```rust
pub type ComposerOptions = serde_json::Map<String, serde_json::Value>;
```

A profile's composer options: a JSON object whose shape the composer's schema declares.

## const `ENDPOINT_TABLE`

`src/settings.rs`

```rust
pub const ENDPOINT_TABLE: crate::ConfigTable = _;
```

Named endpoint entries: `[endpoint.<name>]`.

## const `IMAGE_MAX_BYTES`

`src/limits.rs`

```rust
pub const IMAGE_MAX_BYTES: u64 = _;
```

Maximum original bytes accepted for one image.

## const `PLUGIN_SETTINGS_TABLE`

`src/registry/configuration.rs`

```rust
pub const PLUGIN_SETTINGS_TABLE: ConfigTable = _;
```

The registry owns the named settings sections of plugins.

## const `PROFILE_TABLE`

`src/settings.rs`

```rust
pub const PROFILE_TABLE: crate::ConfigTable = _;
```

Named profile entries: `[profile.<name>]`.

## const `RECENT_REGISTRATIONS`

`src/registry/status.rs`

```rust
pub const RECENT_REGISTRATIONS: usize = 5;
```

The bounded registration history shown with current state.

## const `TRIAL_CALLS`

`src/limits.rs`

```rust
pub const TRIAL_CALLS: u32 = 5;
```

Successful trial calls required before a generation becomes a rollback target.
