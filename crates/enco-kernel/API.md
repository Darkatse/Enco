<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_kernel`

`src/lib.rs`

Session execution through explicit ports.

- enum `Accepted` — Whether an Event was newly accepted.
- struct `Activation` — One selected generation and where it came from; the running and registered layers share it.
- struct `ActivationWrite` — Mechanical recovery moves references; it never creates new intent.
- struct `ActiveConfig` — The active configuration generation: registered content plus the declarations the current host derives from it. The declarations are private to the registry, so only the registry builds one, from a single evaluation, and the parts cannot drift apart.
- enum `Answer` — Finite probabilities in [0, 1], or a refusal for this question. Choice and score distributions follow label order and sum to one, allowing rounding.
- enum `ArtifactSource` — Newly built bytes or a content address already known to the store.
- struct `Budget` — Context limits supplied by configuration.
- struct `CallContext` — Invocation identity, cooperative cancellation and the inline result budget.
- enum `Cause` — A failure attributed to the part of the intent it concerns.
- struct `CodeProvenance` — Where a generation's code came from; settings-only generations follow their code generation.
- enum `CodeSource` — How a generation's code entered the registry.
- struct `Commit` — An atomic append of Log facts and consumption of their corresponding Inbox rows.
- struct `Completion` — Provider output with host-assigned call identities.
- struct `CompletionEndpoint` — A profile's endpoint with the context limits required by completion.
- enum `ComposeError` — A policy could not produce a valid request.
- struct `ComposeInput` — Fixed Round inputs, with the selected budgets and recorded recovery facts for this composition.
- enum `Composition` — A ready reply request or a request to summarize earlier complete Rounds first.
- struct `ConfigStatus` — Configuration is one intent document; a rollback changes only its active reference.
- enum `ConfigTable` — The structural unit owned by one configuration declaration.
- struct `ConfigurationSource` — Parsed configuration paired with the exact input retained by the host.
- struct `ConnectionWrite` — State returned to the connection owner for atomic acceptance.
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
- struct `LoadedTools` — Tool declarations are evaluated once with the generation's accepted configuration.
- enum `Need` — A use's interface and behavior requirements, shared by wiring checks and real probes.
- struct `NodeRecord` — Persistent node identity and recovery policy.
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
- struct `Use` — One configured user's endpoint and requirements.
- enum `Verdict` — One completed invocation's verdict, before caller-specific result handling.
- struct `View` — The one published snapshot of routing and registered configuration.
- trait `Clock` — Time boundary used for reminders and observation, never Log ordering.
- trait `Composer` — Pure policy boundary: identical inputs must produce identical plans.
- trait `ContextSource` — Supplies current candidate content; never called in safe mode.
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
- const `ENDPOINT_TABLE` — Named endpoint entries: `[endpoint.<name>]`.
- const `IMAGE_MAX_BYTES` — Maximum original bytes accepted for one image.
- const `PLUGIN_SETTINGS_TABLE` — The registry owns the named settings sections of plugins.
- const `PROFILE_TABLE` — Named profile entries: `[profile.<name>]`.
- const `RECENT_REGISTRATIONS` — The bounded registration history shown with current state.
- const `TRIAL_CALLS` — Successful trial calls required before a generation becomes a rollback target.

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

Implements `Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`.

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

Implements `Clone`, `Serialize`.

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

Implements `Clone`, `Default`.

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

The active configuration generation: registered content plus the declarations the current
host derives from it. The declarations are private to the registry, so only the registry
builds one, from a single evaluation, and the parts cannot drift apart.

- `generation` — Active configuration generation.
- `document` — Registered document; never read from the intent file during use.
- `kernel` — Parsed kernel-owned tables.

Implements `Clone`.

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

Implements `Debug`.

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

Implements `Clone`, `Copy`, `Debug`.

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
        endpoint: String,
        plugin: PluginName,
    },
    PluginUnavailable {
        user: String,
        endpoint: String,
        plugin: PluginName,
    },
    Use {
        user: String,
        endpoint: String,
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
- `UnknownPlugin` — A configured endpoint names a plugin that does not run.
  - `user` — Configured consumer.
  - `endpoint` — Endpoint named by that consumer.
  - `plugin` — Plugin named by the endpoint.
- `PluginUnavailable` — A configured endpoint names a plugin that runs but cannot serve calls.
  - `user` — Configured consumer.
  - `endpoint` — Endpoint named by that consumer.
  - `plugin` — Plugin named by the endpoint.
- `Use` — A configured user's endpoint cannot be served.
  - `user` — Configured consumer.
  - `endpoint` — Endpoint named by that consumer.
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

Implements `Clone`, `Debug`, `Display`, `Error`, `Serialize`.

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

Implements `Clone`, `Copy`, `Eq`, `PartialEq`, `Serialize`.

## enum `CodeSource`

`src/registry/intent.rs`

```rust
pub enum CodeSource {
    Factory,
    Directory {
        registration: RegistrationId,
    },
    Artifact,
}
```

How a generation's code entered the registry.

- `Factory` — Code shipped with this host, registered or not.
- `Directory` — A registered source directory.
  - `registration` — Registration that registered the directory.
- `Artifact` — A directly supplied component.

Implements `Clone`, `Copy`, `Eq`, `PartialEq`, `Serialize`.

## struct `Commit`

`src/ports/store.rs`

```rust
pub struct Commit {
    pub entries: Vec<Entry>,
}
```

An atomic append of Log facts and consumption of their corresponding Inbox rows.

- `entries` — Ordered facts to append.

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

Implements `Clone`, `Debug`.

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

Implements `Clone`.

## enum `ComposeError`

`src/ports/composer.rs`

```rust
pub enum ComposeError {
    CompactionUnavailable,
    ContextOverflow {
        needed: u32,
        window: u32,
    },
    Invalid(String),
}
```

A policy could not produce a valid request.

- `CompactionUnavailable` — The request fits only after compaction, and no compaction candidate is usable.
- `ContextOverflow` — Even the smallest valid request exceeds the model's window.
  - `needed` — Estimated total size.
  - `window` — Configured context window.
- `Invalid` — Policy inputs could not be composed.

Implements `Debug`, `Display`, `Error`.

## struct `ComposeInput`

`src/ports/composer.rs`

```rust
pub struct ComposeInput {
    pub now: DateTime<Utc>,
    pub timezone: Tz,
    pub session: SessionRecord,
    pub transcript: Transcript,
    pub previous_plan: Option<ContextPlan>,
    pub context: Contribution,
    pub unavailable: Vec<(String, Failure)>,
    pub tools: Vec<(CapabilityId, ToolSpec)>,
    pub safe_mode: Option<SafeMode>,
    pub reply: ReplyTarget,
    pub compaction: Option<Budget>,
    pub compactions_left: u32,
    pub image_paths: std::collections::HashMap<ContentHash, std::path::PathBuf>,
}
```

Fixed Round inputs, with the selected budgets and recorded recovery facts for this composition.

- `now` — UTC instant sampled once for this Round.
- `timezone` — Owner's time zone, shared with recurring schedules.
- `session` — Session metadata sampled for this Round.
- `transcript` — Canonical history after the latest compaction.
- `previous_plan` — Most recent reply plan; the composer decides whether to reuse it.
- `context` — Candidate content from the configured sources, in priority order, plus omissions the
  kernel records itself, such as an unreadable previous plan.
- `unavailable` — Published plugins excluded from the Round's capabilities and their current reasons.
- `tools` — All capabilities which may be disclosed this Round.
- `safe_mode` — Why this Round uses factory instructions and lifeline tools, if applicable.
- `reply` — The reply model this composition is for.
- `compaction` — The summarizing model's limits; absent when no compaction candidate is usable.
- `compactions_left` — Compactions this Round still allows; at zero the composer must return a plan or fail.
- `image_paths` — Local blob addresses supplied by the store for image annotations.

Implements `Clone`.

## enum `Composition`

`src/ports/composer.rs`

```rust
pub enum Composition {
    Plan(ContextPlan),
    Compact {
        upto: LogPos,
        plan: ContextPlan,
    },
}
```

A ready reply request or a request to summarize earlier complete Rounds first.

- `Plan` — Ready for a reply attempt.
- `Compact` — Summarize an inclusive prefix ending at a complete Round.
  - `upto` — Inclusive compaction boundary.
  - `plan` — Tool-free request to generate the summary.

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

Implements `Clone`, `Copy`.

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
    pub settlement: Option<DeliverySettlement>,
}
```

State returned to the connection owner for atomic acceptance.

- `key` — Adapter-declared protocol and account identity.
- `state` — Complete state snapshot owned by this connection.
- `settlement` — Final settlement of a Log-derived delivery, if any.

Implements `Clone`.

## struct `ContextError`

`src/ports/context.rs`

```rust
pub struct ContextError(pub String);
```

A context source failed to supply its authoritative content.

Implements `Debug`, `Display`, `Error`.

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
    pub origin: Option<String>,
    pub required: bool,
    pub header: String,
    pub prefix: String,
}
```

A declared credential; transport validates placement and alone reads its value.

- `name` — Name used in the owner's secret mapping.
- `origin` — Fixed service origin, or the service selected by the caller.
- `required` — Whether the mapping must be present.
- `header` — Header populated by the transport.
- `prefix` — Literal prefix before the secret value.

Implements `Clone`, `Serialize`.

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

Implements `Serialize`.

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

Implements `Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`.

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

Implements `Clone`.

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

Implements `Clone`.

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

Implements `Clone`.

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

Implements `Clone`.

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

Implements `Clone`.

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

Implements `Clone`, `Serialize`.

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

Implements `Debug`, `Display`, `Error`, `From<StoreError>`.

## struct `Inspection`

`src/inspect.rs`

```rust
pub struct Inspection {
    pub attempt: AttemptId,
    pub purpose: AttemptPurpose,
    pub composer: CodeRef,
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
- `composer` — Code which produced the frozen plan.
- `provider` — Provider generation to which the recorded request was bound.
- `settings` — Recorded invocation parameters, excluding the credential itself.
- `request` — Exact canonical request at this Attempt's Log position.
- `plan` — Frozen plan, including each inline message's sources and all recorded omissions.
- `result` — Recorded settlement, absent while the Attempt is unsettled.

Implements `Debug`, `Deserialize<'de>`, `Serialize`.

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
        endpoint: String,
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
- `Use` — A configured endpoint as one user needs it.
  - `user` — Configured consumer, such as `profile default.reply`.
  - `endpoint` — Endpoint named by that consumer.
- `Table` — A top-level configuration table, including a name for named entries.
  - `table` — Table such as `memory` or `endpoint.chat`.
- `Factory` — Code shipped with this host; registration cannot change it.
  - `plugin` — Plugin whose factory code is at fault.
- `Environment` — A requirement the owner provides outside the intent, such as a secret value.
  - `plugin` — Plugin missing the requirement.
- `Plugin` — A plugin that cannot run; its own status says why.
  - `plugin` — The plugin.

Implements `Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`.

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

### `Kernel::submit`

```rust
pub async fn submit(&self, session: SessionId, event_id: EventId, text: String, images: Vec<Image>) -> Result<Accepted, KernelError>
```

Persist input before waking its owner; duplicate IDs remain harmless.

### `Kernel::accept`

```rust
pub async fn accept(&self, events: &[Event], connection: Option<&ConnectionWrite>) -> Result<Vec<Accepted>, KernelError>
```

Accept inputs and connection progress before waking the affected Session owners.

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

Implements `Drop`.

## struct `KernelDeps`

`src/kernel.rs`

```rust
pub struct KernelDeps {
    pub store: std::sync::Arc<dyn Store>,
    pub registry: std::sync::Arc<Registry>,
    pub composer: std::sync::Arc<dyn Composer>,
    pub context: Vec<std::sync::Arc<dyn ContextSource>>,
    pub tools: Vec<std::sync::Arc<dyn Tool>>,
    pub lifeline: Vec<String>,
    pub clock: std::sync::Arc<dyn Clock>,
}
```

Ports assembled once by the application; no service locator is involved.

- `store` — Durable authority.
- `registry` — Sole owner of plugin generations and published exports.
- `composer` — Pure context policy.
- `context` — Context sources in contribution order.
- `tools` — Host capabilities exposed through the ordinary Tool port.
- `lifeline` — Tool names required by Sessions with requires_lifeline.
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

Implements `Debug`, `Display`, `Error`, `From<RegistryError>`, `From<ScheduleError>`, `From<StoreError>`.

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
pub fn from_document(document: &Value) -> Result<Self, SettingsError>
```

Parse and validate only the kernel-owned tables.

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

Implements `Clone`.

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

Implements `Clone`, `Debug`.

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

Implements `Clone`, `Debug`, `Display`, `Error`, `Serialize`.

## struct `Loaded`

`src/ports/runtime.rs`

```rust
pub struct Loaded {
    pub summary: String,
    pub config_schema: serde_json::Value,
    pub credentials: Vec<Credential>,
    pub unavailable: Option<enco_core::Failure>,
    pub lifecycle: std::sync::Arc<dyn Lifecycle>,
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
- `unavailable` — A missing environment requirement; the code still loads and retains its health.
- `lifecycle` — Self-check required before a deployment is admitted.
- `completion` — Completion interface when supplied by the component.
- `embedding` — Embedding interface when supplied by the component.
- `decision` — Decision interface when supplied by the component.
- `tools` — Declared tool definitions and their common invocation interface.

Implements `Clone`.

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

Implements `Clone`.

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

Implements `Clone`, `Eq`, `PartialEq`.

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

Implements `Clone`, `Debug`.

## enum `PluginChange`

`src/registry/request.rs`

```rust
pub enum PluginChange {
    Factory,
    Code {
        source: ContentHash,
        artifact: ArtifactSource,
    },
    Artifact(Vec<u8>),
    Remove,
    Rename(PluginName),
}
```

One plugin's contribution to an atomic registration.

- `Factory` — Current embedded code for a factory plugin without a source directory.
- `Code` — A component built from source intent.
  - `source` — Source tree already persisted by the host.
  - `artifact` — Compiled component offered for validation.
- `Artifact` — A component supplied independently of source intent.
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
    Artifact {
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
- `Artifact` — A component file was accepted independently of source.
  - `generation` — Selected generation and its current outcome.

### `PluginChangeKind::generation`

```rust
pub fn generation(&self) -> Option<&GenerationRecord>
```

Generation selected by this change, including a reused factory generation.

Implements `Clone`, `Serialize`.

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

Implements `Serialize`.

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

Implements `Serialize`.

## struct `Profile`

`src/profile.rs`

```rust
pub struct Profile {
    pub reply: Endpoints,
    pub compaction: Endpoints,
    pub requires_lifeline: bool,
}
```

Session policy sampled once at the start of a Round.

- `reply` — Ordered candidates for ordinary replies.
- `compaction` — Ordered candidates for summarizing history.
- `requires_lifeline` — Whether every reply plan must disclose all lifeline tools.

Implements `Clone`.

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

Implements `Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`.

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

Implements `Clone`, `Debug`, `Deserialize<'de>`, `PartialEq`, `Serialize`.

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

Implements `Clone`, `Debug`.

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

Implements `Clone`, `Debug`.

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

- `User` — A configured user, through an endpoint naming the plugin.
  - `user` — The user, such as `profile default.reply`.
- `Settings` — The plugin's own `[plugin.<name>]` table.

Implements `Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`.

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

Implements `Serialize`.

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

Implements `Clone`.

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
- `code` — Last accepted code intent; direct artifacts do not introduce source intent.
- `activation` — Generation selected by the latest accepted activation, or this host's unregistered factory code.
- `factory` — Host-owned code and its follow decision, when the name is reserved.

### `RegisteredPlugin::source`

```rust
pub fn source(&self) -> Option<ContentHash>
```

Address of the accepted source directory, absent when following factory code.

Implements `Clone`, `Serialize`.

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

Implements `Clone`, `Serialize`.

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

Implements `Clone`.

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

Implements `Serialize`.

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

Implements `Clone`.

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
    pub owners: Vec<TableOwner>,
    pub host_tool_names: Vec<String>,
    pub clock: std::sync::Arc<dyn Clock>,
}
```

Durable registry dependencies assembled by the application.

- `store` — Sole persistence port for registry facts and component artifacts.
- `runtime` — Component compilation and export discovery.
- `factory` — Host-reserved identities and embedded bytes.
- `owners` — Host-owned tables and their declaration functions.
- `host_tool_names` — Names from the native tool declarations assembled by the composition root.
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

Implements `Debug`, `Display`, `Error`, `From<RegistryError>`, `From<StoreError>`.

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

Implements `Clone`, `Default`.

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

Implements `Clone`.

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

Implements `Default`.

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

Implements `Serialize`.

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

Implements `Serialize`.

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

Implements `Clone`.

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

Implements `Debug`, `Display`, `Error`, `From<ScheduleError>`, `From<StoreError>`.

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

Implements `Clone`, `Debug`.

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

Implements `Clone`.

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

Implements `Debug`.

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

Implements `Serialize`.

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

Implements `Clone`, `Debug`, `Display`, `Error`, `Serialize`.

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

Implements `Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`.

## struct `Status`

`src/kernel.rs`

```rust
pub struct Status {
    pub timezone: Tz,
    pub node: NodeId,
    pub safe_mode: bool,
    pub composer: CodeRef,
    pub profiles: Vec<String>,
    pub sessions: Vec<SessionStatus>,
    pub scheduler_stopped: Option<String>,
}
```

Current node and Session execution state.

- `timezone` — Owner's timezone from the selected configuration.
- `node` — Persistent node identity.
- `safe_mode` — Policy observed at the next Round boundary.
- `composer` — Configured composer code.
- `profiles` — Available profile names; endpoint credentials are never exposed.
- `sessions` — State of every registered Session actor.
- `scheduler_stopped` — Failure that stopped the Scheduler, if any.

Implements `Serialize`.

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
- `Artifact` — Component bytes are missing or do not match their address.
- `UnknownGeneration` — A referenced registry record does not exist.
- `SchemaVersion` — The authority uses another schema version; opening it never deletes or migrates data.
  - `found` — Version recorded in the database.
  - `expected` — Version understood by this binary.
- `Backend` — The storage engine or filesystem operation failed.

Implements `Debug`, `Display`, `Error`, `From<StoreError>`.

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
- `declare` — Parse the owned tables and project their endpoint requirements.

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

Implements `Clone`, `Debug`, `Display`, `Eq`, `PartialEq`, `Serialize`.

## struct `Transcript`

`src/ports/composer.rs`

```rust
pub struct Transcript {
    pub summary: Option<String>,
    pub items: Vec<TranscriptItem>,
    pub round_ends: Vec<LogPos>,
    pub run_ends: Vec<(LogPos, RunEnd)>,
}
```

Model-facing projection of immutable Log facts.

- `summary` — Latest replacement for summarized history.
- `items` — Canonical messages still visible after that summary, in ascending Log position order.
- `round_ends` — Complete Round boundaries eligible for compaction, in ascending Log position order.
- `run_ends` — Run outcomes after the summary, in Log order; the composer decides how to describe them.

Implements `Clone`, `Debug`, `Default`.

## struct `TranscriptItem`

`src/ports/composer.rs`

```rust
pub struct TranscriptItem {
    pub pos: LogPos,
    pub message: Message,
    pub generation: Option<GenerationId>,
    pub event: Option<Event>,
}
```

One canonical message and its immutable source.

- `pos` — Position of the originating fact.
- `message` — Canonical message used when resolving a plan reference.
- `generation` — Provider generation of the producing Attempt, present only for Assistant messages.
- `event` — Consumed Inbox Event, present only for inputs; the composer narrates its circumstances.

Implements `Clone`, `Debug`.

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

Implements `Clone`, `Serialize`.

## struct `UnservedUse`

`src/registry/status.rs`

```rust
pub struct UnservedUse {
    pub user: String,
    pub endpoint: String,
    pub causes: Vec<Cause>,
}
```

One current configured use that cannot be served by the selected exports.

- `user` — Configured consumer, such as `memory.embedding`.
- `endpoint` — Endpoint named by that consumer.
- `causes` — Why the selected exports cannot serve it.

Implements `Serialize`.

## struct `Use`

`src/registry.rs`

```rust
pub struct Use {
    pub user: String,
    pub endpoint: crate::Endpoint,
    pub need: Need,
}
```

One configured user's endpoint and requirements.

- `user` — Human-readable user, such as `profile default.reply`.
- `endpoint` — Resolved service and invocation parameters.
- `need` — Required interface and behavior.

Implements `Clone`.

## enum `Verdict`

`src/registry.rs`

```rust
pub enum Verdict {
    Ok,
    Failed(Failure),
}
```

One completed invocation's verdict, before caller-specific result handling.

- `Ok` — A valid plugin result, regardless of the caller's subsequent policy checks.
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

### `Composer::code`

```rust
fn code(&self) -> CodeRef
```

Composer code recorded with every attempt.

### `Composer::compose`

```rust
fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError>
```

Select, order and budget content without IO or hidden inputs.

## trait `ContextSource`

`src/ports/context.rs`

```rust
pub trait ContextSource: Send + Sync
```

Supplies current candidate content; never called in safe mode.

### `ContextSource::contribute`

```rust
async fn contribute(&self, query: &'life1 ContextQuery) -> Result<Contribution, ContextError>
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
async fn decide(&self, settings: &'life1 ProviderSettings, state: String, questions: Vec<Question>) -> Result<Vec<Answer>, Failure>
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
async fn embed(&self, settings: &'life1 ProviderSettings, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure>
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
async fn complete(&self, settings: &'life1 ProviderSettings, request: ProviderRequest) -> Result<Completion, Failure>
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
async fn load(&self, artifact: &'life1 [u8], settings: &'life2 PluginSettings) -> Result<Loaded, LoadError>
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
async fn session_by_name(&self, name: &'life1 str) -> Result<Option<SessionRecord>, StoreError>
```

Look up a Session without creating it.

### `Store::ensure_session`

```rust
async fn ensure_session(&self, name: &'life1 str, created_at: DateTime<Utc>) -> Result<SessionRecord, StoreError>
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

### `Store::accept`

```rust
async fn accept(&self, events: &'life1 [Event], connection: Option<&'life2 ConnectionWrite>) -> Result<Vec<Accepted>, StoreError>
```

Atomically accept Events, connection state and an optional final delivery outcome.

### `Store::connection`

```rust
async fn connection(&self, key: &'life1 str) -> Result<Option<serde_json::Value>, StoreError>
```

Read the state owned by one channel connection.

### `Store::delivery_failures`

```rust
async fn delivery_failures(&self, key: &'life1 str) -> Result<Vec<DeliverySettlement>, StoreError>
```

Read recent failed or uncertain deliveries in reverse commit order.

### `Store::pending`

```rust
async fn pending(&self, session: SessionId) -> Result<Vec<Event>, StoreError>
```

Read unconsumed inputs in acceptance order.

### `Store::insert_schedule`

```rust
async fn insert_schedule(&self, schedule: &'life1 Schedule) -> Result<(), StoreError>
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
async fn fire_schedule(&self, expected_last: Option<DateTime<Utc>>, done: bool, event: &'life1 Event) -> Result<(), StoreError>
```

Advance an active Schedule from expected_last and accept its Event atomically.
Mark it done when no occurrence remains.

### `Store::set_profile`

```rust
async fn set_profile(&self, id: SessionId, profile: &'life1 str) -> Result<(), StoreError>
```

Select the profile under the registry committer, effective at the next Round.

### `Store::registry`

```rust
async fn registry(&self) -> Result<RegistryState, StoreError>
```

Read registration history and the references restored at startup.

### `Store::record_factories`

```rust
async fn record_factories(&self, generations: &'life1 [GenerationRecord]) -> Result<(), StoreError>
```

Record embedded factory content without activating it or creating a registration.

### `Store::register`

```rust
async fn register(&self, write: &'life1 RegistrationWrite) -> Result<(), StoreError>
```

Atomically accept one registration, its content and its runtime references.

### `Store::promote`

```rust
async fn promote(&self, generation: GenerationId) -> Result<(), StoreError>
```

Promote an observed trial without changing its identity or rollback target.

### `Store::activate`

```rust
async fn activate(&self, change: &'life1 ActivationWrite) -> Result<(), StoreError>
```

Atomically move runtime references and record attributable failures and a notice.

### `Store::retain_artifacts`

```rust
async fn retain_artifacts(&self, hashes: &'life1 std::collections::HashSet<ContentHash>) -> Result<(), StoreError>
```

Remove component files which no runtime reference retains.

### `Store::put_artifact`

```rust
async fn put_artifact(&self, bytes: &'life1 [u8]) -> Result<ContentHash, StoreError>
```

Persist immutable component bytes separately from removable history blobs.

### `Store::artifact`

```rust
async fn artifact(&self, hash: &'life1 ContentHash) -> Result<Vec<u8>, StoreError>
```

Read and verify component bytes by content address.

### `Store::put_blob`

```rust
async fn put_blob(&self, bytes: &'life1 [u8]) -> Result<ContentHash, StoreError>
```

Persist immutable bytes before recording their content address.

### `Store::get_blob`

```rust
async fn get_blob(&self, hash: &'life1 ContentHash) -> Result<Vec<u8>, StoreError>
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

Execute after dispatch checks disclosure, checks the arguments against the tool's schema
(dropping optional properties given as null) and confirms the call may start.
Respond to cancellation and wait until started work is quiescent before returning.
Pageable results should fit ctx.result_budget and explain how to continue.

## trait `Tools`

`src/ports/runtime.rs`

```rust
pub trait Tools: Send + Sync
```

A plugin's tools interface, independent of the registry-assigned code identity.

### `Tools::call`

```rust
async fn call(&self, ctx: CallContext, spec: &'life1 ToolSpec, args: serde_json::Map<String, serde_json::Value>) -> Outcome
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
