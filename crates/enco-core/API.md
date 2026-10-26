<!-- Generated from rustdoc by `cargo xtask docs`. Do not edit. -->
# crate `enco_core`

`src/lib.rs`

Durable vocabulary shared by the kernel, host and local protocol.

- module `code` — Stable failure codes shared with WIT plugins.
- struct `ArgumentError` — Why a call's arguments do not fit the tool's schema, naming the argument.
- enum `Arguments` — The single normalization step for raw tool arguments, used by dispatch.
- struct `AttemptId` — Identity of one provider attempt, including failed attempts.
- enum `AttemptPurpose` — Whether the model is replying or summarizing prior rounds.
- enum `AttemptResult` — The durable settlement of a provider attempt.
- struct `Binding` — Execution owner and fencing generation; a single node uses itself at epoch one.
- struct `CallId` — Host-assigned identity of one proposed tool invocation.
- struct `CapabilityId` — A node-qualified capability, displayed as `name@node`; the model sees only `name`.
- enum `CodeIntent` — The owner's choice of code source, independent of the resulting component bytes.
- enum `CodeRef` — Identifies the exact code used by an invocation.
- struct `ConfigGeneration` — An immutable registered configuration document.
- struct `ConfigurationRef` — The identity and registration of a configuration, without copying its document.
- struct `ContentHash` — BLAKE3 address of immutable bytes, encoded as lowercase hexadecimal.
- struct `ContextPlan` — The complete request plan, recorded by content address before sending.
- struct `Delivery` — A logical channel delivery referencing its original Log fact.
- struct `DeliverySettlement` — Final outcome, committed atomically with the connection's outbound progress.
- struct `DisclosedTool` — A tool as a plan disclosed it to the model, kept verbatim.
- enum `Effect` — Describes external effects so recovery can settle an interrupted invocation honestly.
- struct `Entry` — One immutable fact at a structurally ordered position.
- enum `EntryBody` — Facts accepted by the Session owner.
- struct `Epoch` — Ownership generation; single-node execution uses one.
- struct `Event` — A deduplicated input delivered through the Inbox.
- enum `EventBody` — Input that requires attention from the agent.
- struct `EventId` — Input identity used for delivery deduplication.
- enum `EventSource` — The producer of an Inbox input.
- struct `Extension` — Opaque provider-owned message fields retained for subsequent requests.
- struct `Failure` — Stable machine-readable classification with contextual diagnostic text.
- struct `FileData` — File bytes handed to the kernel at settlement, or resolved for one delivery.
- struct `GenerationId` — Registry-global generation number, ordered by commit rather than time.
- struct `GenerationRecord` — One immutable generation identity, with a mutable availability status.
- enum `GenerationStatus` — Eligibility for use and automatic recovery.
- struct `Image` — Recorded image metadata; the kernel stores the bytes before accepting this reference.
- struct `ImageData` — Bytes resolved from a recorded image for one frozen Provider request.
- enum `ImageFormat` — Encodings shared by the supported image input protocols.
- enum `Initiator` — Provenance of an accepted change; Agent reasoning remains in its Session log.
- struct `InputSchema` — A tool's argument schema: a closed object in the canonical subset that the plugin contract states for `tool-spec`.
- enum `IntentPath` — An addressable configuration file or plugin directory in the Enco home.
- struct `InvalidIntentPath` — A path that cannot be addressed by the development commands.
- struct `InvalidPluginName` — A plugin name outside the public naming grammar.
- enum `InvalidReason` — A reason that cannot be stored as a single bounded line.
- struct `LogPos` — Structural Log ordering; wall clocks never determine this order.
- struct `Memory` — A self-contained fact about the owner; its current authority is the memory database.
- struct `MemoryId` — Identity of an editable memory.
- struct `Message` — Provider-independent message; this shape is part of the Log format.
- enum `Modality` — Non-text input accepted by a completion endpoint.
- struct `NodeId` — Persistent identity of an Enco installation.
- struct `Occurrence` — Last delivered occurrence; the next one is derived rather than stored.
- struct `Omission` — A visible explanation of content excluded from this request.
- enum `Origin` — Source of a registered generation.
- enum `Outcome` — Known success, known failure, or an effect whose outcome is uncertain.
- struct `Outgoing` — Content addressed to the owner: Markdown text and at most one file.
- struct `OutgoingFile` — A recorded file; the kernel stores the bytes before accepting this reference.
- enum `Part` — Typed content of a canonical message.
- enum `PlanItem` — Inline content or a reference to an immutable canonical message.
- struct `PluginActivation` — A current or retained code reference, distinct from the generation's creation provenance.
- struct `PluginId` — Persistent identity assigned when a plugin name is registered.
- struct `PluginName` — A validated directory and routing name for one plugin.
- struct `PluginRecord` — Mutable references owned exclusively by the registry.
- struct `PluginSettings` — The complete `[plugin.<name>]` table. Only `config` is visible to the plugin.
- struct `ProviderSettings` — Per-call provider settings recorded before invocation. Credential values belong only to the host transport.
- struct `Reason` — A nonempty, bounded line explaining an intentional change.
- struct `RegisteredCode` — One accepted code activation.
- struct `RegisteredName` — One accepted name change.
- struct `RegistrationId` — Registry-global registration number; one accepted change may contain several generations.
- struct `RegistrationRecord` — One accepted external change, also the unit of owner rollback.
- enum `Role` — The speaker in a canonical model message.
- struct `RolledBack` — A committed automatic rollback, also accepted as a Session input when applicable.
- enum `RoundEnd` — Why this Round stopped.
- struct `RoundId` — Identity of a model request and its tool settlements.
- enum `RunEnd` — Why this Run stopped.
- struct `RunId` — Identity of one Run of a Session.
- struct `Schedule` — A durable sequence of reminders delivered to a Session Inbox.
- struct `ScheduleId` — Identity of a durable schedule.
- enum `ScheduleRule` — One instant or a recurring pattern interpreted in local time.
- enum `ScheduleState` — Schedule lifetime; firing and Inbox acceptance are atomic.
- struct `SchemaError` — Why a declared schema is outside the canonical subset, and where, as a JSON Pointer.
- struct `Seq` — Sequence assigned by a Session actor, starting at one.
- struct `SessionId` — Durable identity of a conversation and its Log.
- struct `SessionRecord` — Persistent Session identity, execution binding and selected profile.
- struct `SessionRef` — A Session's stable identity with its resolved name for inspection.
- enum `Settlement` — Durable execution outcome; result text is stored once in the Log entry's content or blob.
- struct `Source` — A source an inline message renders, named and hashed by the composer that wrote it.
- enum `StopReason` — Provider-reported completion boundary.
- struct `ToolCall` — A proposed invocation; arguments are normalized only when dispatched.
- struct `ToolResult` — Model-facing settlement linked to the original provider call ID.
- struct `ToolSpec` — A model-facing tool contract; its name is unique within a Round snapshot.
- struct `Unviewable` — An attachment the owner sent but this host could not make visible as an image.
- enum `UnviewableReason` — Expected attachment failures remain part of their input Event.
- struct `Usage` — Token counts reported by the provider.
- fn `parse_arguments` — Parse a call's arguments into the tool's argument type.
- const `CONFIGURATION_FILE` — The owner's configuration file, relative to the Enco home.
- const `DEFAULT_PROFILE` — Profile selected when a Session is created; every node configuration must define it.
- const `PLUGINS_DIRECTORY` — The directory holding one source directory per plugin, relative to the Enco home.
- const `REASON_PLACEHOLDER` — Reserved marker in suggested calls; it is never an acceptable registration reason.

Re-exports `chrono::DateTime`, `chrono::FixedOffset`, `chrono::Utc`, `chrono_tz::Tz`.

## struct `ArgumentError`

`src/schema.rs`

```rust
pub struct ArgumentError(/* private field */);
```

Why a call's arguments do not fit the tool's schema, naming the argument.

Implements:

```rust
impl Clone for ArgumentError
impl Debug for ArgumentError
impl Display for ArgumentError
impl Eq for ArgumentError
impl Error for ArgumentError
impl PartialEq for ArgumentError
```

## enum `Arguments`

`src/tool.rs`

```rust
pub enum Arguments {
    Object(serde_json::Map<String, serde_json::Value>),
    Invalid {
        raw: String,
    },
}
```

The single normalization step for raw tool arguments, used by dispatch.

- `Object` — Normalized object arguments, not yet checked against the tool's schema.
- `Invalid` — Input that cannot be interpreted as an argument object.
  - `raw` — Original invalid input retained for diagnosis.

### `Arguments::parse`

```rust
pub fn parse(raw: &str) -> Self
```

Normalize empty, null, object and once-double-encoded object arguments.

Implements:

```rust
impl Clone for Arguments
impl Debug for Arguments
impl<'de> Deserialize<'de> for Arguments
impl PartialEq for Arguments
impl Serialize for Arguments
```

## struct `AttemptId`

`src/ids.rs`

```rust
pub struct AttemptId(/* private field */);
```

Identity of one provider attempt, including failed attempts.

### `AttemptId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for AttemptId
impl Copy for AttemptId
impl Debug for AttemptId
impl Default for AttemptId
impl<'de> Deserialize<'de> for AttemptId
impl Display for AttemptId
impl Eq for AttemptId
impl FromStr for AttemptId
impl Hash for AttemptId
impl JsonSchema for AttemptId
impl Ord for AttemptId
impl PartialEq for AttemptId
impl PartialOrd for AttemptId
impl Serialize for AttemptId
```

## enum `AttemptPurpose`

`src/entry.rs`

```rust
pub enum AttemptPurpose {
    Reply,
    Compaction,
}
```

Whether the model is replying or summarizing prior rounds.

- `Reply` — A reply whose tool calls may be dispatched.
- `Compaction` — A summary request whose output replaces earlier context.

Implements:

```rust
impl Clone for AttemptPurpose
impl Copy for AttemptPurpose
impl Debug for AttemptPurpose
impl<'de> Deserialize<'de> for AttemptPurpose
impl Eq for AttemptPurpose
impl PartialEq for AttemptPurpose
impl Serialize for AttemptPurpose
```

## enum `AttemptResult`

`src/entry.rs`

```rust
pub enum AttemptResult {
    Completed {
        message: crate::Message,
        usage: Usage,
        stop: StopReason,
    },
    Failed {
        failure: crate::Failure,
    },
}
```

The durable settlement of a provider attempt.

- `Completed` — The operation completed normally.
  - `message` — Accepted Assistant output, including tool proposals and adapter extensions.
  - `usage` — Provider-reported token usage.
  - `stop` — Provider-reported stop condition.
- `Failed` — The operation stopped with a known failure.
  - `failure` — Classification and details of the failure.

### `AttemptResult::failure`

```rust
pub fn failure(&self, purpose: AttemptPurpose) -> Option<Failure>
```

The failure of this invocation for its purpose, retaining successful wire results as facts.

Implements:

```rust
impl Clone for AttemptResult
impl Debug for AttemptResult
impl<'de> Deserialize<'de> for AttemptResult
impl PartialEq for AttemptResult
impl Serialize for AttemptResult
```

## struct `Binding`

`src/session.rs`

```rust
pub struct Binding {
    pub node: crate::NodeId,
    pub epoch: crate::Epoch,
}
```

Execution owner and fencing generation; a single node uses itself at epoch one.

- `node` — Node currently authorized to execute this Session.
- `epoch` — Ownership generation.

Implements:

```rust
impl Clone for Binding
impl Debug for Binding
impl<'de> Deserialize<'de> for Binding
impl PartialEq for Binding
impl Serialize for Binding
```

## struct `CallId`

`src/ids.rs`

```rust
pub struct CallId(/* private field */);
```

Host-assigned identity of one proposed tool invocation.

### `CallId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for CallId
impl Copy for CallId
impl Debug for CallId
impl Default for CallId
impl<'de> Deserialize<'de> for CallId
impl Display for CallId
impl Eq for CallId
impl FromStr for CallId
impl Hash for CallId
impl JsonSchema for CallId
impl Ord for CallId
impl PartialEq for CallId
impl PartialOrd for CallId
impl Serialize for CallId
```

## struct `CapabilityId`

`src/capability.rs`

```rust
pub struct CapabilityId {
    pub node: crate::NodeId,
    pub name: String,
}
```

A node-qualified capability, displayed as `name@node`; the model sees only `name`.

- `node` — Node exporting this capability.
- `name` — Tool name within the exporting node.

Implements:

```rust
impl Clone for CapabilityId
impl Debug for CapabilityId
impl<'de> Deserialize<'de> for CapabilityId
impl Display for CapabilityId
impl PartialEq for CapabilityId
impl Serialize for CapabilityId
```

## enum `CodeIntent`

`src/registration.rs`

```rust
pub enum CodeIntent {
    Directory {
        source: crate::ContentHash,
    },
    Factory,
}
```

The owner's choice of code source, independent of the resulting component bytes.

- `Directory` — Build and maintain the source directory in the home.
  - `source` — Content-addressed source tree owned by the store.
- `Factory` — Follow the implementation supplied by the host.

Implements:

```rust
impl Clone for CodeIntent
impl Copy for CodeIntent
impl Debug for CodeIntent
impl<'de> Deserialize<'de> for CodeIntent
impl Eq for CodeIntent
impl PartialEq for CodeIntent
impl Serialize for CodeIntent
```

## enum `CodeRef`

`src/capability.rs`

```rust
pub enum CodeRef {
    Generation {
        id: crate::GenerationId,
    },
    Native {
        name: String,
        version: String,
    },
}
```

Identifies the exact code used by an invocation.

- `Generation` — Code and settings registered together as a plugin generation.
  - `id` — Immutable registry identity.
- `Native` — Code linked into the host binary.
  - `name` — Identity of the linked native capability or policy.
  - `version` — Version of the linked native crate.

Implements:

```rust
impl Clone for CodeRef
impl Debug for CodeRef
impl<'de> Deserialize<'de> for CodeRef
impl PartialEq for CodeRef
impl Serialize for CodeRef
```

## struct `ConfigGeneration`

`src/generation.rs`

```rust
pub struct ConfigGeneration {
    pub id: crate::GenerationId,
    pub registration: crate::RegistrationId,
    pub source: crate::ContentHash,
    pub document: serde_json::Value,
    pub previous: Option<crate::GenerationId>,
    pub created_at: crate::DateTime<crate::Utc>,
}
```

An immutable registered configuration document.

- `id` — Global registry sequence number, shared with plugin generations.
- `registration` — Accepted change which produced this generation.
- `source` — Content address of the exact configuration file, including comments.
- `document` — Complete configuration content; declarations are derived by its owners.
- `previous` — Configuration which was active immediately before this one was registered.
- `created_at` — Observation time for display, never ordering.

Implements:

```rust
impl Clone for ConfigGeneration
impl Debug for ConfigGeneration
impl<'de> Deserialize<'de> for ConfigGeneration
impl PartialEq for ConfigGeneration
impl Serialize for ConfigGeneration
```

## struct `ConfigurationRef`

`src/generation.rs`

```rust
pub struct ConfigurationRef {
    pub generation: crate::GenerationId,
    pub registration: crate::RegistrationId,
}
```

The identity and registration of a configuration, without copying its document.

- `generation` — Selected configuration generation.
- `registration` — Registration that accepted the configuration.

### `ConfigurationRef::of`

```rust
pub fn of(configuration: &ConfigGeneration) -> Self
```

The reference to one configuration generation.

Implements:

```rust
impl Clone for ConfigurationRef
impl Copy for ConfigurationRef
impl Debug for ConfigurationRef
impl Eq for ConfigurationRef
impl PartialEq for ConfigurationRef
impl Serialize for ConfigurationRef
```

## struct `ContentHash`

`src/hash.rs`

```rust
pub struct ContentHash(/* private field */);
```

BLAKE3 address of immutable bytes, encoded as lowercase hexadecimal.

### `ContentHash::of`

```rust
pub fn of(bytes: &[u8]) -> Self
```

Address these exact bytes.

Implements:

```rust
impl Clone for ContentHash
impl Copy for ContentHash
impl Debug for ContentHash
impl<'de> Deserialize<'de> for ContentHash
impl Display for ContentHash
impl Eq for ContentHash
impl FromStr for ContentHash
impl Hash for ContentHash
impl PartialEq for ContentHash
impl Serialize for ContentHash
```

## struct `ContextPlan`

`src/plan.rs`

```rust
pub struct ContextPlan {
    pub items: Vec<PlanItem>,
    pub tools: Vec<(crate::CapabilityId, DisclosedTool)>,
    pub max_output_tokens: Option<u32>,
    pub omitted: Vec<Omission>,
}
```

The complete request plan, recorded by content address before sending.

- `items` — Messages in request order.
- `tools` — Disclosed capabilities together with their frozen definitions.
- `max_output_tokens` — Requested output budget; absent means provider default.
- `omitted` — Reasons content was not included in this request.

### `ContextPlan::hash`

```rust
pub fn hash(&self) -> Result<ContentHash, serde_json::Error>
```

The plan's identity: the address of its complete JSON, whatever form stores it.

Implements:

```rust
impl Clone for ContextPlan
impl Debug for ContextPlan
impl<'de> Deserialize<'de> for ContextPlan
impl PartialEq for ContextPlan
impl Serialize for ContextPlan
```

## struct `Delivery`

`src/delivery.rs`

```rust
pub struct Delivery {
    pub session: crate::SessionId,
    pub pos: crate::LogPos,
    pub target: String,
}
```

A logical channel delivery referencing its original Log fact.

- `session` — Session containing the original outgoing message, reply or terminal notice.
- `pos` — ToolCallSettled, RoundEnded or RunEnded position which caused this delivery.
- `target` — Protocol-local destination.

Implements:

```rust
impl Clone for Delivery
impl Debug for Delivery
impl<'de> Deserialize<'de> for Delivery
impl PartialEq for Delivery
impl Serialize for Delivery
```

## struct `DeliverySettlement`

`src/delivery.rs`

```rust
pub struct DeliverySettlement {
    pub delivery: Delivery,
    pub outcome: crate::Settlement,
    pub at: crate::DateTime<crate::Utc>,
}
```

Final outcome, committed atomically with the connection's outbound progress.

- `delivery` — Original Log reference and destination.
- `outcome` — One final outcome for the entire logical delivery.
- `at` — Observation time for display, never ordering.

Implements:

```rust
impl Clone for DeliverySettlement
impl Debug for DeliverySettlement
impl<'de> Deserialize<'de> for DeliverySettlement
impl PartialEq for DeliverySettlement
impl Serialize for DeliverySettlement
```

## struct `DisclosedTool`

`src/plan.rs`

```rust
pub struct DisclosedTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub effect: crate::Effect,
}
```

A tool as a plan disclosed it to the model, kept verbatim.

A recorded schema is read back as it was sent and never checked again, so plans recorded under
earlier rules stay readable.

- `name` — Name the model proposes calls by.
- `description` — Instructions the model read.
- `input_schema` — The argument schema as sent.
- `effect` — Mutation semantics declared with the tool.

Implements:

```rust
impl Clone for DisclosedTool
impl Debug for DisclosedTool
impl<'de> Deserialize<'de> for DisclosedTool
impl From<&ToolSpec> for DisclosedTool
impl PartialEq for DisclosedTool
impl Serialize for DisclosedTool
```

## enum `Effect`

`src/tool.rs`

```rust
pub enum Effect {
    ReadOnly,
    Idempotent,
    SideEffect,
}
```

Describes external effects so recovery can settle an interrupted invocation honestly.

- `ReadOnly` — No effects beyond content committed with settlement; interruption can be retried.
- `Idempotent` — Repeating the same operation produces the same intended effect.
- `SideEffect` — An external mutation which must not be blindly repeated.

Implements:

```rust
impl Clone for Effect
impl Copy for Effect
impl Debug for Effect
impl<'de> Deserialize<'de> for Effect
impl Eq for Effect
impl PartialEq for Effect
impl Serialize for Effect
```

## struct `Entry`

`src/entry.rs`

```rust
pub struct Entry {
    pub pos: crate::LogPos,
    pub at: crate::DateTime<crate::Utc>,
    pub body: EntryBody,
}
```

One immutable fact at a structurally ordered position.

- `pos` — Position assigned by the Session owner.
- `at` — Observation time for display, never for ordering.
- `body` — Accepted input or execution transition at this Log position.

Implements:

```rust
impl Clone for Entry
impl Debug for Entry
impl<'de> Deserialize<'de> for Entry
impl PartialEq for Entry
impl Serialize for Entry
```

## enum `EntryBody`

`src/entry.rs`

```rust
pub enum EntryBody {
    EventConsumed {
        event: crate::Event,
    },
    RunStarted {
        run: crate::RunId,
    },
    RoundStarted {
        run: crate::RunId,
        round: crate::RoundId,
        safe_mode: bool,
    },
    AttemptStarted {
        round: crate::RoundId,
        attempt: crate::AttemptId,
        purpose: AttemptPurpose,
        plan: crate::ContentHash,
        composer: crate::GenerationId,
        provider: crate::GenerationId,
        settings: crate::ProviderSettings,
    },
    AttemptSettled {
        attempt: crate::AttemptId,
        result: AttemptResult,
    },
    ToolCallStarted {
        round: crate::RoundId,
        call: crate::CallId,
        capability: crate::CapabilityId,
        code: crate::CodeRef,
        effect: crate::Effect,
    },
    ToolCallSettled {
        call: crate::CallId,
        outcome: crate::Settlement,
        content: String,
        full: Option<crate::ContentHash>,
        images: Vec<crate::Image>,
        outgoing: Option<crate::Outgoing>,
    },
    RoundEnded {
        round: crate::RoundId,
        end: RoundEnd,
    },
    RunEnded {
        run: crate::RunId,
        end: RunEnd,
    },
    Compacted {
        upto: crate::LogPos,
        summary: String,
        attempt: crate::AttemptId,
    },
}
```

Facts accepted by the Session owner.

- `EventConsumed` — Accept an Inbox Event into Session history and consume its row in the same Commit.
  - `event` — The complete Event accepted from this Session’s Inbox.
- `RunStarted` — A new Run was accepted.
  - `run` — Run containing this fact.
- `RoundStarted` — A new execution boundary was accepted.
  - `run` — Run containing this fact.
  - `round` — Round containing this fact.
  - `safe_mode` — Whether this Round uses only the lifeline.
- `AttemptStarted` — Committed before calling the Provider; `plan` addresses the recorded ContextPlan.
  - `round` — Round containing this fact.
  - `attempt` — Provider attempt associated with this fact.
  - `purpose` — Purpose of this provider request.
  - `plan` — Address of the frozen ContextPlan.
  - `composer` — Composer generation which constructed the frozen plan.
  - `provider` — Provider generation used by this Attempt.
  - `settings` — Exact per-call parameters; credentials are never recorded.
- `AttemptSettled` — A provider attempt completed or failed.
  - `attempt` — Provider attempt associated with this fact.
  - `result` — Durable settlement returned by the provider.
- `ToolCallStarted` — Committed before execution. Calls rejected by dispatch do not receive this entry.
  - `round` — Round containing this fact.
  - `call` — Host identity of the proposed tool call.
  - `capability` — Node-qualified capability bound to the call.
  - `code` — Implementation bound to this invocation.
  - `effect` — Mutation semantics used when recovering an unsettled call.
- `ToolCallSettled` — One settlement per call proposed by a completed reply, including calls never dispatched.
  - `call` — Host identity of the proposed tool call.
  - `outcome` — Known or uncertain result of execution.
  - `content` — Exact model-visible result text.
  - `full` — Address of the complete result when its inline text was truncated.
  - `images` — Images stored by the settlement before this entry was committed.
  - `outgoing` — Owner-directed content accepted with this settlement.
- `RoundEnded` — All proposed calls have been settled and the Round has ended.
  - `round` — Round containing this fact.
  - `end` — Reason execution stopped.
- `RunEnded` — This Run will perform no more work.
  - `run` — Run containing this fact.
  - `end` — Reason execution stopped.
- `Compacted` — Replace the Transcript prefix through `upto` with `summary`; retain the original Log entries.
  - `upto` — Inclusive Log position hidden by the summary.
  - `summary` — Model-generated replacement for the summarized history.
  - `attempt` — Provider attempt associated with this fact.

### `EntryBody::canonical_message`

```rust
pub fn canonical_message(&self, purpose: Option<AttemptPurpose>, call: Option<&ToolCall>) -> Option<Message>
```

Resolve a durable entry's model-facing form using its recorded associations.
This mapping is part of the persistent format, not composer policy.

Implements:

```rust
impl Clone for EntryBody
impl Debug for EntryBody
impl<'de> Deserialize<'de> for EntryBody
impl PartialEq for EntryBody
impl Serialize for EntryBody
```

## struct `Epoch`

`src/ids.rs`

```rust
pub struct Epoch(pub u64);
```

Ownership generation; single-node execution uses one.

Implements:

```rust
impl Clone for Epoch
impl Copy for Epoch
impl Debug for Epoch
impl<'de> Deserialize<'de> for Epoch
impl Eq for Epoch
impl Hash for Epoch
impl Ord for Epoch
impl PartialEq for Epoch
impl PartialOrd for Epoch
impl Serialize for Epoch
```

## struct `Event`

`src/event.rs`

```rust
pub struct Event {
    pub id: crate::EventId,
    pub session: crate::SessionId,
    pub source: EventSource,
    pub body: EventBody,
    pub received_at: crate::DateTime<crate::Utc>,
}
```

A deduplicated input delivered through the Inbox.

- `id` — Sender-assigned ID used to deduplicate repeated deliveries across restarts.
- `session` — Destination Session whose Inbox accepts this Event.
- `source` — Producer used to distinguish interactive input from system notices.
- `body` — Content that needs a Session Run.
- `received_at` — Reception time for display, not Inbox ordering.

### `Event::canonical_message`

```rust
pub fn canonical_message(&self) -> Message
```

The sole mapping from Inbox input to a model message: the content its producer supplied.

The composer narrates the circumstances, such as arrival and planned times. Input without
text, such as a photo alone, has no text part.

Implements:

```rust
impl Clone for Event
impl Debug for Event
impl<'de> Deserialize<'de> for Event
impl PartialEq for Event
impl Serialize for Event
```

## enum `EventBody`

`src/event.rs`

```rust
pub enum EventBody {
    GenerationRolledBack {
        rollback: crate::RolledBack,
        text: String,
    },
    UserMessage {
        text: String,
        images: Vec<crate::Image>,
        unviewable: Vec<crate::Unviewable>,
    },
    Reminder {
        schedule: crate::ScheduleId,
        due_at: crate::DateTime<crate::Utc>,
        skipped: u32,
        text: String,
    },
}
```

Input that requires attention from the agent.

- `GenerationRolledBack` — A failed trial generation and its committed replacement.
  - `rollback` — Committed recovery facts.
  - `text` — Notice written by the Registry when the rollback committed.
- `UserMessage` — A message from the owner.
  - `text` — Text supplied by the owner.
  - `images` — Images stored before this Event was accepted.
  - `unviewable` — Attachments whose content could not be accepted as images.
- `Reminder` — A reminder reaching its scheduled time.
  - `schedule` — The Schedule which produced this input.
  - `due_at` — Planned time in UTC; this occurrence's identity together with the schedule.
  - `skipped` — Earlier missed occurrences omitted when delivering only the latest one.
  - `text` — Reminder text copied from its Schedule.

Implements:

```rust
impl Clone for EventBody
impl Debug for EventBody
impl<'de> Deserialize<'de> for EventBody
impl PartialEq for EventBody
impl Serialize for EventBody
```

## struct `EventId`

`src/ids.rs`

```rust
pub struct EventId(/* private field */);
```

Input identity used for delivery deduplication.

### `EventId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for EventId
impl Copy for EventId
impl Debug for EventId
impl Default for EventId
impl<'de> Deserialize<'de> for EventId
impl Display for EventId
impl Eq for EventId
impl FromStr for EventId
impl Hash for EventId
impl JsonSchema for EventId
impl Ord for EventId
impl PartialEq for EventId
impl PartialOrd for EventId
impl Serialize for EventId
```

## enum `EventSource`

`src/event.rs`

```rust
pub enum EventSource {
    Cli,
    Channel {
        channel: String,
        account: String,
        conversation: String,
        sender: String,
    },
    Scheduler,
    Registry,
}
```

The producer of an Inbox input.

- `Cli` — Input submitted through the native interface.
- `Channel` — Interactive input from a channel connection.
  - `channel` — Protocol declared by the channel adapter.
  - `account` — Account identity within the protocol.
  - `conversation` — Conversation to which default replies return.
  - `sender` — Authenticated sender within the protocol.
- `Scheduler` — Input generated by a durable schedule.
- `Registry` — Notice generated by automatic plugin recovery.

Implements:

```rust
impl Clone for EventSource
impl Debug for EventSource
impl<'de> Deserialize<'de> for EventSource
impl PartialEq for EventSource
impl Serialize for EventSource
```

## struct `Extension`

`src/message.rs`

```rust
pub struct Extension {
    pub data: serde_json::Value,
}
```

Opaque provider-owned message fields retained for subsequent requests.

- `data` — Opaque JSON; the producing Attempt's generation identifies its owner.

Implements:

```rust
impl Clone for Extension
impl Debug for Extension
impl<'de> Deserialize<'de> for Extension
impl PartialEq for Extension
impl Serialize for Extension
```

## struct `Failure`

`src/tool.rs`

```rust
pub struct Failure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
```

Stable machine-readable classification with contextual diagnostic text.

- `code` — Stable failure category shared across execution boundaries.
- `message` — Context explaining what failed and how to recover.
- `retryable` — Whether repeating this request later may succeed.

Implements:

```rust
impl Clone for Failure
impl Debug for Failure
impl<'de> Deserialize<'de> for Failure
impl PartialEq for Failure
impl Serialize for Failure
```

## struct `FileData`

`src/outgoing.rs`

```rust
pub struct FileData {
    pub name: String,
    pub data: Vec<u8>,
}
```

File bytes handed to the kernel at settlement, or resolved for one delivery.

- `name` — Filename without its original directory.
- `data` — Original bytes, encoded only by the channel adapter.

Implements:

```rust
impl Clone for FileData
impl Debug for FileData
impl<'de> Deserialize<'de> for FileData
impl Eq for FileData
impl PartialEq for FileData
impl Serialize for FileData
```

## struct `GenerationId`

`src/ids.rs`

```rust
pub struct GenerationId(pub u64);
```

Registry-global generation number, ordered by commit rather than time.

Implements:

```rust
impl Clone for GenerationId
impl Copy for GenerationId
impl Debug for GenerationId
impl<'de> Deserialize<'de> for GenerationId
impl Display for GenerationId
impl Eq for GenerationId
impl Hash for GenerationId
impl Ord for GenerationId
impl PartialEq for GenerationId
impl PartialOrd for GenerationId
impl Serialize for GenerationId
```

## struct `GenerationRecord`

`src/generation.rs`

```rust
pub struct GenerationRecord {
    pub id: crate::GenerationId,
    pub plugin: crate::PluginId,
    pub artifact: crate::ContentHash,
    pub settings: PluginSettings,
    pub origin: Origin,
    pub status: GenerationStatus,
    pub failure: Option<crate::Failure>,
    pub created_at: crate::DateTime<crate::Utc>,
}
```

One immutable generation identity, with a mutable availability status.

- `id` — Registry-assigned structural sequence number.
- `plugin` — Plugin whose history contains this generation.
- `artifact` — Content-addressed component bytes.
- `settings` — Owner-selected plugin configuration and credential names, fixed together with the code.
- `origin` — How this generation entered the registry.
- `status` — Whether this generation remains eligible for use.
- `failure` — The reason this generation was marked failed; absent for trial and healthy records.
- `created_at` — Observation time for display, never ordering.

Implements:

```rust
impl Clone for GenerationRecord
impl Debug for GenerationRecord
impl<'de> Deserialize<'de> for GenerationRecord
impl PartialEq for GenerationRecord
impl Serialize for GenerationRecord
```

## enum `GenerationStatus`

`src/generation.rs`

```rust
pub enum GenerationStatus {
    Trial,
    Healthy,
    Failed,
}
```

Eligibility for use and automatic recovery.

- `Trial` — Active under observation; not yet eligible as a rollback target.
- `Healthy` — Eligible to become active or serve as a rollback target.
- `Failed` — Unavailable; redeploying its bytes creates a new identity.

Implements:

```rust
impl Clone for GenerationStatus
impl Copy for GenerationStatus
impl Debug for GenerationStatus
impl<'de> Deserialize<'de> for GenerationStatus
impl Eq for GenerationStatus
impl PartialEq for GenerationStatus
impl Serialize for GenerationStatus
```

## struct `Image`

`src/image.rs`

```rust
pub struct Image {
    pub format: ImageFormat,
    pub size: u64,
    pub content: crate::ContentHash,
}
```

Recorded image metadata; the kernel stores the bytes before accepting this reference.

- `format` — Detected encoding.
- `size` — Original byte length, before protocol encoding.
- `content` — Address of the original bytes in the blob store.

Implements:

```rust
impl Clone for Image
impl Debug for Image
impl<'de> Deserialize<'de> for Image
impl Eq for Image
impl PartialEq for Image
impl Serialize for Image
```

## struct `ImageData`

`src/image.rs`

```rust
pub struct ImageData {
    pub format: ImageFormat,
    pub data: Vec<u8>,
}
```

Bytes resolved from a recorded image for one frozen Provider request.

- `format` — Recorded encoding.
- `data` — Original bytes, encoded only by the Provider.

Implements:

```rust
impl Clone for ImageData
impl Debug for ImageData
impl<'de> Deserialize<'de> for ImageData
impl Eq for ImageData
impl PartialEq for ImageData
impl Serialize for ImageData
```

## enum `ImageFormat`

`src/image.rs`

```rust
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}
```

Encodings shared by the supported image input protocols.

- `Png` — PNG encoding.
- `Jpeg` — JPEG encoding.
- `Webp` — WebP encoding.

### `ImageFormat::detect`

```rust
pub fn detect(bytes: &[u8]) -> Option<Self>
```

Recognize an encoding by its file signature, independently of filenames and MIME labels.

### `ImageFormat::mime`

```rust
pub fn mime(self) -> &'static str
```

MIME label used at protocol boundaries.

Implements:

```rust
impl Clone for ImageFormat
impl Copy for ImageFormat
impl Debug for ImageFormat
impl<'de> Deserialize<'de> for ImageFormat
impl Eq for ImageFormat
impl PartialEq for ImageFormat
impl Serialize for ImageFormat
```

## enum `Initiator`

`src/registration.rs`

```rust
pub enum Initiator<S = crate::SessionId> {
    Owner {
        reason: Option<Reason>,
    },
    Host,
    Agent {
        session: S,
        call: crate::CallId,
        reason: Reason,
    },
}
```

Provenance of an accepted change; Agent reasoning remains in its Session log.

- `Owner` — An explicit owner command.
  - `reason` — The owner's explanation, if any.
- `Host` — Initial registration or following a new host's factory code.
- `Agent` — A tool call made by an Agent.
  - `session` — Session containing the reasoning and tool invocation.
  - `call` — Invocation which requested registration.
  - `reason` — Explanation given before changing registered behavior.

### `Initiator::reason`

```rust
pub fn reason(&self) -> Option<&Reason>
```

The caller's explanation; host changes have no caller-provided reason.

Implements:

```rust
impl<S: Clone> Clone for Initiator<S>
impl<S: Debug> Debug for Initiator<S>
impl<'de, S> Deserialize<'de> for Initiator<S> where S: Deserialize<'de>
impl<S: Eq> Eq for Initiator<S>
impl<S: PartialEq> PartialEq for Initiator<S>
impl<S> Serialize for Initiator<S> where S: Serialize
```

## struct `InputSchema`

`src/schema.rs`

```rust
pub struct InputSchema(/* private field */);
```

A tool's argument schema: a closed object in the canonical subset that the plugin contract
states for `tool-spec`.

### `InputSchema::parse`

```rust
pub fn parse(value: &Value) -> Result<Self, SchemaError>
```

Parse a declared schema. The root must be a closed object without a description or default:
the tool's description has its own field.

### `InputSchema::of`

```rust
pub fn of<T: JsonSchema>() -> Result<Self, SchemaError>
```

The schema of `T`, generated from the type and its field documentation.

### `InputSchema::empty`

```rust
pub fn empty() -> Self
```

The schema of a tool that takes no arguments.

### `InputSchema::to_value`

```rust
pub fn to_value(&self) -> Value
```

The schema in canonical JSON form.

### `InputSchema::accept`

```rust
pub fn accept(&self, arguments: Map<String, Value>) -> Result<Map<String, Value>, ArgumentError>
```

Check a call's arguments and return them in canonical form.

Optional properties given as `null` are removed, since `null` there means "not given", and
integers written as `5.0` become `5`.

Implements:

```rust
impl Clone for InputSchema
impl Debug for InputSchema
impl PartialEq for InputSchema
```

## enum `IntentPath`

`src/intent.rs`

```rust
pub enum IntentPath {
    Configuration,
    Plugin(PluginName),
}
```

An addressable configuration file or plugin directory in the Enco home.

- `Configuration` — The owner's complete configuration document.
- `Plugin` — Source files under the plugin's current registered name.

### `IntentPath::locate`

```rust
pub fn locate(&self, home: &Path) -> PathBuf
```

Where this path lives under the Enco home.

Implements:

```rust
impl Clone for IntentPath
impl Debug for IntentPath
impl<'de> Deserialize<'de> for IntentPath
impl Display for IntentPath
impl Eq for IntentPath
impl From<IntentPath> for String
impl FromStr for IntentPath
impl JsonSchema for IntentPath
impl Ord for IntentPath
impl PartialEq for IntentPath
impl PartialOrd for IntentPath
impl Serialize for IntentPath
```

## struct `InvalidIntentPath`

`src/intent.rs`

```rust
pub struct InvalidIntentPath(/* private field */);
```

A path that cannot be addressed by the development commands.

Implements:

```rust
impl Debug for InvalidIntentPath
impl Display for InvalidIntentPath
impl Error for InvalidIntentPath
```

## struct `InvalidPluginName`

`src/intent.rs`

```rust
pub struct InvalidPluginName(/* private field */);
```

A plugin name outside the public naming grammar.

Implements:

```rust
impl Debug for InvalidPluginName
impl Display for InvalidPluginName
impl Error for InvalidPluginName
```

## enum `InvalidReason`

`src/registration.rs`

```rust
pub enum InvalidReason {
    Placeholder,
    Line,
}
```

A reason that cannot be stored as a single bounded line.

- `Placeholder` — The caller has not replaced the suggested-call marker.
- `Line` — A reason must fit on one bounded, nonempty line.

Implements:

```rust
impl Debug for InvalidReason
impl Display for InvalidReason
impl Error for InvalidReason
```

## struct `LogPos`

`src/ids.rs`

```rust
pub struct LogPos {
    pub epoch: Epoch,
    pub seq: Seq,
}
```

Structural Log ordering; wall clocks never determine this order.

- `epoch` — Ownership generation.
- `seq` — Sequence within the generation.

Implements:

```rust
impl Clone for LogPos
impl Copy for LogPos
impl Debug for LogPos
impl<'de> Deserialize<'de> for LogPos
impl Eq for LogPos
impl Hash for LogPos
impl Ord for LogPos
impl PartialEq for LogPos
impl PartialOrd for LogPos
impl Serialize for LogPos
```

## struct `Memory`

`src/memory.rs`

```rust
pub struct Memory {
    pub id: crate::MemoryId,
    pub text: String,
    pub pinned: bool,
    pub created_at: crate::DateTime<crate::Utc>,
    pub updated_at: crate::DateTime<crate::Utc>,
    pub rev: u64,
}
```

A self-contained fact about the owner; its current authority is the memory database.

- `id` — Identity retained when the fact is corrected, and removed when it is forgotten.
- `text` — The current statement; recall returns this text rather than indexed copies.
- `pinned` — Prioritize this fact each Round without retrieval; budget omissions remain explicit.
- `created_at` — Creation time in UTC.
- `updated_at` — Time of the latest authority update.
- `rev` — Starts at one and increments on each correction, allowing the index to detect stale content.

Implements:

```rust
impl Clone for Memory
impl Debug for Memory
impl<'de> Deserialize<'de> for Memory
impl PartialEq for Memory
impl Serialize for Memory
```

## struct `MemoryId`

`src/ids.rs`

```rust
pub struct MemoryId(/* private field */);
```

Identity of an editable memory.

### `MemoryId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for MemoryId
impl Copy for MemoryId
impl Debug for MemoryId
impl Default for MemoryId
impl<'de> Deserialize<'de> for MemoryId
impl Display for MemoryId
impl Eq for MemoryId
impl FromStr for MemoryId
impl Hash for MemoryId
impl JsonSchema for MemoryId
impl Ord for MemoryId
impl PartialEq for MemoryId
impl PartialOrd for MemoryId
impl Serialize for MemoryId
```

## struct `Message`

`src/message.rs`

```rust
pub struct Message<I = crate::Image> {
    pub role: Role,
    pub parts: Vec<Part<I>>,
}
```

Provider-independent message; this shape is part of the Log format.

- `role` — Speaker of this message.
- `parts` — Content in provider order.

### `Message::text`

```rust
pub fn text(role: Role, text: impl Into<String>) -> Self
```

Construct a text-only message.

### `Message::tool_calls`

```rust
pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall>
```

Proposed calls in provider order.

### `Message::joined_text`

```rust
pub fn joined_text(&self) -> String
```

Join visible text, excluding provider extensions and tool calls.

Implements:

```rust
impl<I: Clone> Clone for Message<I>
impl<I: Debug> Debug for Message<I>
impl<'de, I> Deserialize<'de> for Message<I> where I: Deserialize<'de>
impl<I: PartialEq> PartialEq for Message<I>
impl<I> Serialize for Message<I> where I: Serialize
```

## enum `Modality`

`src/image.rs`

```rust
pub enum Modality {
    Image,
}
```

Non-text input accepted by a completion endpoint.

- `Image` — Still-image input.

Implements:

```rust
impl Clone for Modality
impl Copy for Modality
impl Debug for Modality
impl<'de> Deserialize<'de> for Modality
impl Eq for Modality
impl Ord for Modality
impl PartialEq for Modality
impl PartialOrd for Modality
impl Serialize for Modality
```

## struct `NodeId`

`src/ids.rs`

```rust
pub struct NodeId(/* private field */);
```

Persistent identity of an Enco installation.

### `NodeId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for NodeId
impl Copy for NodeId
impl Debug for NodeId
impl Default for NodeId
impl<'de> Deserialize<'de> for NodeId
impl Display for NodeId
impl Eq for NodeId
impl FromStr for NodeId
impl Hash for NodeId
impl JsonSchema for NodeId
impl Ord for NodeId
impl PartialEq for NodeId
impl PartialOrd for NodeId
impl Serialize for NodeId
```

## struct `Occurrence`

`src/session.rs`

```rust
pub struct Occurrence {
    pub due_at: crate::DateTime<crate::Utc>,
    pub event: crate::EventId,
}
```

Last delivered occurrence; the next one is derived rather than stored.

- `due_at` — Planned time, independent of when the scheduler actually delivered it.
- `event` — Event accepted into the destination Inbox.

Implements:

```rust
impl Clone for Occurrence
impl Copy for Occurrence
impl Debug for Occurrence
impl<'de> Deserialize<'de> for Occurrence
impl PartialEq for Occurrence
impl Serialize for Occurrence
```

## struct `Omission`

`src/plan.rs`

```rust
pub struct Omission {
    pub source: String,
    pub reason: String,
}
```

A visible explanation of content excluded from this request.

- `source` — Identifier of the omitted content, such as `memory:<id>`.
- `reason` — Explanation visible during request inspection.

Implements:

```rust
impl Clone for Omission
impl Debug for Omission
impl<'de> Deserialize<'de> for Omission
impl PartialEq for Omission
impl Serialize for Omission
```

## enum `Origin`

`src/generation.rs`

```rust
pub enum Origin {
    Factory,
    Code {
        registration: crate::RegistrationId,
    },
    Settings {
        registration: crate::RegistrationId,
        based_on: crate::GenerationId,
    },
}
```

Source of a registered generation.

- `Factory` — Component embedded in the host binary.
- `Code` — Accepted code intent, from source or the current host's factory implementation.
  - `registration` — Accepted change that supplied this code.
- `Settings` — New settings paired with the currently active code.
  - `registration` — Accepted change that supplied these settings.
  - `based_on` — Generation whose code is reused.

### `Origin::registration`

```rust
pub fn registration(self) -> Option<RegistrationId>
```

The accepted change that produced this content; factory content belongs to the host.

Implements:

```rust
impl Clone for Origin
impl Copy for Origin
impl Debug for Origin
impl<'de> Deserialize<'de> for Origin
impl Eq for Origin
impl PartialEq for Origin
impl Serialize for Origin
```

## enum `Outcome`

`src/tool.rs`

```rust
pub enum Outcome {
    Ok {
        value: serde_json::Value,
        images: Vec<Vec<u8>>,
        outgoing: Option<crate::Outgoing<crate::FileData>>,
    },
    Failed {
        failure: Failure,
    },
    Unknown {
        failure: Failure,
    },
}
```

Known success, known failure, or an effect whose outcome is uncertain.

- `Ok` — The operation returned a known result.
  - `value` — Result returned by the tool.
  - `images` — Image bytes handed to the kernel's shared ingestion boundary at settlement.
  - `outgoing` — Owner-directed content accepted atomically with this call's settlement.
- `Failed` — The operation failed with known effects, including cases that never began execution.
  - `failure` — Classification and details of the failure.
- `Unknown` — Effects may already have occurred; never retry automatically.
  - `failure` — Classification and details of the failure.

Implements:

```rust
impl Clone for Outcome
impl Debug for Outcome
impl<'de> Deserialize<'de> for Outcome
impl From<&Outcome> for Settlement
impl PartialEq for Outcome
impl Serialize for Outcome
```

## struct `Outgoing`

`src/outgoing.rs`

```rust
pub struct Outgoing<F = OutgoingFile> {
    pub text: String,
    pub file: Option<F>,
}
```

Content addressed to the owner: Markdown text and at most one file.

- `text` — Markdown authored by the sender.
- `file` — Optional attachment, recorded by reference or carried as bytes at a boundary.

Implements:

```rust
impl<F: Clone> Clone for Outgoing<F>
impl<F: Debug> Debug for Outgoing<F>
impl<'de, F> Deserialize<'de> for Outgoing<F> where F: Deserialize<'de>
impl<F: Eq> Eq for Outgoing<F>
impl<F: PartialEq> PartialEq for Outgoing<F>
impl<F> Serialize for Outgoing<F> where F: Serialize
```

## struct `OutgoingFile`

`src/outgoing.rs`

```rust
pub struct OutgoingFile {
    pub name: String,
    pub size: u64,
    pub content: crate::ContentHash,
}
```

A recorded file; the kernel stores the bytes before accepting this reference.

- `name` — Filename without its original directory.
- `size` — Original byte length.
- `content` — Address of the original bytes in the blob store.

Implements:

```rust
impl Clone for OutgoingFile
impl Debug for OutgoingFile
impl<'de> Deserialize<'de> for OutgoingFile
impl Eq for OutgoingFile
impl PartialEq for OutgoingFile
impl Serialize for OutgoingFile
```

## enum `Part`

`src/message.rs`

```rust
pub enum Part<I = crate::Image> {
    Text {
        text: String,
    },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Extension(Extension),
    Image(I),
}
```

Typed content of a canonical message.

- `Text` — UTF-8 text visible to the model.
  - `text` — Text visible to the model as part of this message.
- `ToolCall` — A proposed invocation; arguments are normalized only when dispatched.
- `ToolResult` — Model-facing settlement linked to the original provider call ID.
- `Extension` — Adapter-specific fields, such as reasoning, replayed only by their originating Provider.
- `Image` — Image input, represented by a recorded reference or resolved request bytes.

### `Part::retype`

```rust
pub fn retype<J>(self) -> Result<Part<J>, I>
```

The same part under another image type; an image is handed back for the caller to convert.

Implements:

```rust
impl<I: Clone> Clone for Part<I>
impl<I: Debug> Debug for Part<I>
impl<'de, I> Deserialize<'de> for Part<I> where I: Deserialize<'de>
impl<I: PartialEq> PartialEq for Part<I>
impl<I> Serialize for Part<I> where I: Serialize
```

## enum `PlanItem`

`src/plan.rs`

```rust
pub enum PlanItem {
    Message {
        message: crate::Message,
        sources: Vec<Source>,
    },
    Log {
        pos: crate::LogPos,
        images: bool,
    },
}
```

Inline content or a reference to an immutable canonical message.

- `Message` — Composer-created content recorded directly in this plan.
  - `message` — Inline content created by the composer, recorded verbatim in the plan.
  - `sources` — The sources this message renders, in rendering order.
- `Log` — Reference to a canonical Log message.
  - `pos` — Entry whose canonical message is resolved from this Session’s Log.
  - `images` — Whether this whole entry's images accompany its text in the request.

Implements:

```rust
impl Clone for PlanItem
impl Debug for PlanItem
impl<'de> Deserialize<'de> for PlanItem
impl PartialEq for PlanItem
impl Serialize for PlanItem
```

## struct `PluginActivation`

`src/registration.rs`

```rust
pub struct PluginActivation {
    pub generation: crate::GenerationId,
    pub registration: Option<crate::RegistrationId>,
}
```

A current or retained code reference, distinct from the generation's creation provenance.

- `generation` — Target code and settings.
- `registration` — Registration which wrote this reference; absent only for direct factory recovery.

Implements:

```rust
impl Clone for PluginActivation
impl Copy for PluginActivation
impl Debug for PluginActivation
impl<'de> Deserialize<'de> for PluginActivation
impl Eq for PluginActivation
impl PartialEq for PluginActivation
impl Serialize for PluginActivation
```

## struct `PluginId`

`src/ids.rs`

```rust
pub struct PluginId(/* private field */);
```

Persistent identity assigned when a plugin name is registered.

### `PluginId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for PluginId
impl Copy for PluginId
impl Debug for PluginId
impl Default for PluginId
impl<'de> Deserialize<'de> for PluginId
impl Display for PluginId
impl Eq for PluginId
impl FromStr for PluginId
impl Hash for PluginId
impl JsonSchema for PluginId
impl Ord for PluginId
impl PartialEq for PluginId
impl PartialOrd for PluginId
impl Serialize for PluginId
```

## struct `PluginName`

`src/intent.rs`

```rust
pub struct PluginName(/* private field */);
```

A validated directory and routing name for one plugin.

### `PluginName::RULE`

```rust
const RULE: &str = "lowercase letters, digits and single hyphens";
```

The naming rule, as messages and argument descriptions state it.

### `PluginName::as_str`

```rust
pub fn as_str(&self) -> &str
```

The validated name.

Implements:

```rust
impl AsRef<str> for PluginName
impl Borrow<str> for PluginName
impl Clone for PluginName
impl Debug for PluginName
impl Deref for PluginName
impl<'de> Deserialize<'de> for PluginName
impl Display for PluginName
impl Eq for PluginName
impl From<PluginName> for String
impl FromStr for PluginName
impl Hash for PluginName
impl JsonSchema for PluginName
impl Ord for PluginName
impl PartialEq for PluginName
impl PartialOrd for PluginName
impl Serialize for PluginName
```

## struct `PluginRecord`

`src/registration.rs`

```rust
pub struct PluginRecord {
    pub id: crate::PluginId,
    pub named_by: Option<crate::RegistrationId>,
    pub active: Option<PluginActivation>,
    pub rollback_target: Option<PluginActivation>,
}
```

Mutable references owned exclusively by the registry.

- `id` — Immutable installation identity; names may change or be reused.
- `named_by` — This identity's current name row, including a row recording removal.
- `active` — Code used for new calls; absence means the plugin is stopped.
- `rollback_target` — Retained healthy code available for mechanical recovery.

Implements:

```rust
impl Clone for PluginRecord
impl Debug for PluginRecord
impl<'de> Deserialize<'de> for PluginRecord
impl Eq for PluginRecord
impl PartialEq for PluginRecord
impl Serialize for PluginRecord
```

## struct `PluginSettings`

`src/generation.rs`

```rust
pub struct PluginSettings {
    pub config: serde_json::Value,
    pub secrets: std::collections::BTreeMap<String, String>,
}
```

The complete `[plugin.<name>]` table. Only `config` is visible to the plugin.

- `config` — Plugin-owned configuration, accepted by its declaration function.
- `secrets` — Credential names mapped to environment variable names, never their values.

Implements:

```rust
impl Clone for PluginSettings
impl Debug for PluginSettings
impl Default for PluginSettings
impl<'de> Deserialize<'de> for PluginSettings where PluginSettings: Default
impl PartialEq for PluginSettings
impl Serialize for PluginSettings
```

## struct `ProviderSettings`

`src/provider.rs`

```rust
pub struct ProviderSettings {
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub options: serde_json::Value,
}
```

Per-call provider settings recorded before invocation. Credential values belong only to the host transport.

- `base_url` — Service endpoint for this call.
- `model` — Service-specific model identifier.
- `api_key_env` — Environment variable the host transport reads at invocation time.
- `options` — Adapter options included in the recorded request parameters.

Implements:

```rust
impl Clone for ProviderSettings
impl Debug for ProviderSettings
impl<'de> Deserialize<'de> for ProviderSettings
impl PartialEq for ProviderSettings
impl Serialize for ProviderSettings
```

## struct `Reason`

`src/registration.rs`

```rust
pub struct Reason(/* private field */);
```

A nonempty, bounded line explaining an intentional change.

### `Reason::MAX_CHARS`

```rust
const MAX_CHARS: usize = 512;
```

Maximum number of Unicode characters in a registration reason.

### `Reason::as_str`

```rust
pub fn as_str(&self) -> &str
```

The original explanation, without normalization.

Implements:

```rust
impl Clone for Reason
impl Debug for Reason
impl<'de> Deserialize<'de> for Reason
impl Display for Reason
impl Eq for Reason
impl FromStr for Reason
impl JsonSchema for Reason
impl PartialEq for Reason
impl Serialize for Reason
```

## struct `RegisteredCode`

`src/registration.rs`

```rust
pub struct RegisteredCode {
    pub generation: crate::GenerationId,
    pub intent: Option<CodeIntent>,
}
```

One accepted code activation.

- `generation` — Code activated by this change.
- `intent` — Code intent accepted here; a settings change leaves it unchanged.

Implements:

```rust
impl Clone for RegisteredCode
impl Debug for RegisteredCode
impl<'de> Deserialize<'de> for RegisteredCode
impl Eq for RegisteredCode
impl PartialEq for RegisteredCode
impl Serialize for RegisteredCode
```

## struct `RegisteredName`

`src/registration.rs`

```rust
pub struct RegisteredName {
    pub name: Option<crate::PluginName>,
    pub previous: Option<crate::RegistrationId>,
}
```

One accepted name change.

- `name` — Directory and routing name, or no name after removal.
- `previous` — Name version actually in effect before this change, for the same identity.

Implements:

```rust
impl Clone for RegisteredName
impl Debug for RegisteredName
impl<'de> Deserialize<'de> for RegisteredName
impl Eq for RegisteredName
impl PartialEq for RegisteredName
impl Serialize for RegisteredName
```

## struct `RegistrationId`

`src/ids.rs`

```rust
pub struct RegistrationId(pub u64);
```

Registry-global registration number; one accepted change may contain several generations.

Implements:

```rust
impl Clone for RegistrationId
impl Copy for RegistrationId
impl Debug for RegistrationId
impl<'de> Deserialize<'de> for RegistrationId
impl Display for RegistrationId
impl Eq for RegistrationId
impl Hash for RegistrationId
impl JsonSchema for RegistrationId
impl Ord for RegistrationId
impl PartialEq for RegistrationId
impl PartialOrd for RegistrationId
impl Serialize for RegistrationId
```

## struct `RegistrationRecord`

`src/registration.rs`

```rust
pub struct RegistrationRecord {
    pub id: crate::RegistrationId,
    pub initiator: Initiator,
    pub names: std::collections::BTreeMap<crate::PluginId, RegisteredName>,
    pub code: std::collections::BTreeMap<crate::PluginId, RegisteredCode>,
    pub created_at: crate::DateTime<crate::Utc>,
}
```

One accepted external change, also the unit of owner rollback.

- `id` — Structural commit order, independent of wall clocks.
- `initiator` — Caller responsible for this accepted change.
- `names` — Name versions written only when an identity acquires, changes or releases its name.
- `code` — Code activations written by this change, independently of name changes.
- `created_at` — Observation time for display.

Implements:

```rust
impl Clone for RegistrationRecord
impl Debug for RegistrationRecord
impl<'de> Deserialize<'de> for RegistrationRecord
impl PartialEq for RegistrationRecord
impl Serialize for RegistrationRecord
```

## enum `Role`

`src/message.rs`

```rust
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}
```

The speaker in a canonical model message.

- `System` — Instructions supplied by the composer.
- `User` — Owner input and contextual messages from event producers or the composer.
- `Assistant` — Model output, including proposed calls.
- `Tool` — Settlement of a proposed call.

Implements:

```rust
impl Clone for Role
impl Copy for Role
impl Debug for Role
impl<'de> Deserialize<'de> for Role
impl Eq for Role
impl PartialEq for Role
impl Serialize for Role
```

## struct `RolledBack`

`src/generation.rs`

```rust
pub struct RolledBack {
    pub plugin: crate::PluginName,
    pub from: crate::GenerationId,
    pub to: Option<crate::GenerationId>,
    pub failure: crate::Failure,
}
```

A committed automatic rollback, also accepted as a Session input when applicable.

- `plugin` — Running plugin name.
- `from` — Failed trial generation.
- `to` — Replacement, or none when no healthy generation remains.
- `failure` — Plugin fault which triggered recovery.

Implements:

```rust
impl Clone for RolledBack
impl Debug for RolledBack
impl<'de> Deserialize<'de> for RolledBack
impl PartialEq for RolledBack
impl Serialize for RolledBack
```

## enum `RoundEnd`

`src/entry.rs`

```rust
pub enum RoundEnd {
    Replied,
    ToolsSettled,
    Interrupted,
    Cancelled,
    Failed {
        failure: crate::Failure,
        generations: Vec<crate::GenerationId>,
    },
}
```

Why this Round stopped.

- `Replied` — A reply without tool calls was recorded.
- `ToolsSettled` — All proposed tool calls have durable outcomes.
- `Interrupted` — The previous process ended before settlement.
- `Cancelled` — Cancellation was requested and work has stopped.
- `Failed` — The operation stopped with a known failure.
  - `failure` — Classification and details of the failure.
  - `generations` — Active configuration and plugin generations when this Round failed, in ascending order.

Implements:

```rust
impl Clone for RoundEnd
impl Debug for RoundEnd
impl<'de> Deserialize<'de> for RoundEnd
impl PartialEq for RoundEnd
impl Serialize for RoundEnd
```

## struct `RoundId`

`src/ids.rs`

```rust
pub struct RoundId(/* private field */);
```

Identity of a model request and its tool settlements.

### `RoundId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

### `RoundId::as_u128`

```rust
pub fn as_u128(self) -> u128
```

The full Round identity used to seed deterministic composition randomness.

Implements:

```rust
impl Clone for RoundId
impl Copy for RoundId
impl Debug for RoundId
impl Default for RoundId
impl<'de> Deserialize<'de> for RoundId
impl Display for RoundId
impl Eq for RoundId
impl FromStr for RoundId
impl Hash for RoundId
impl JsonSchema for RoundId
impl Ord for RoundId
impl PartialEq for RoundId
impl PartialOrd for RoundId
impl Serialize for RoundId
```

## enum `RunEnd`

`src/entry.rs`

```rust
pub enum RunEnd {
    Completed,
    BudgetExhausted,
    Interrupted,
    Cancelled,
    Failed {
        failure: crate::Failure,
    },
}
```

Why this Run stopped.

- `Completed` — The operation completed normally.
- `BudgetExhausted` — The Run exhausted its Round allowance.
- `Interrupted` — The previous process ended before settlement.
- `Cancelled` — Cancellation was requested and work has stopped.
- `Failed` — The operation stopped with a known failure.
  - `failure` — Classification and details of the failure.

Implements:

```rust
impl Clone for RunEnd
impl Debug for RunEnd
impl<'de> Deserialize<'de> for RunEnd
impl PartialEq for RunEnd
impl Serialize for RunEnd
```

## struct `RunId`

`src/ids.rs`

```rust
pub struct RunId(/* private field */);
```

Identity of one Run of a Session.

### `RunId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for RunId
impl Copy for RunId
impl Debug for RunId
impl Default for RunId
impl<'de> Deserialize<'de> for RunId
impl Display for RunId
impl Eq for RunId
impl FromStr for RunId
impl Hash for RunId
impl JsonSchema for RunId
impl Ord for RunId
impl PartialEq for RunId
impl PartialOrd for RunId
impl Serialize for RunId
```

## struct `Schedule`

`src/session.rs`

```rust
pub struct Schedule {
    pub id: crate::ScheduleId,
    pub session: crate::SessionId,
    pub rule: ScheduleRule,
    pub message: String,
    pub created_at: crate::DateTime<crate::Utc>,
    pub state: ScheduleState,
    pub last: Option<Occurrence>,
}
```

A durable sequence of reminders delivered to a Session Inbox.

- `id` — Identity used to cancel this schedule and link its delivered Events.
- `session` — Destination Session for the reminder Event.
- `rule` — Rule from which future occurrences are derived.
- `message` — Reminder text delivered when due.
- `created_at` — Creation time in UTC.
- `state` — Current schedule lifetime state.
- `last` — Most recent occurrence atomically accepted into the Inbox.

Implements:

```rust
impl Clone for Schedule
impl Debug for Schedule
impl<'de> Deserialize<'de> for Schedule
impl PartialEq for Schedule
impl Serialize for Schedule
```

## struct `ScheduleId`

`src/ids.rs`

```rust
pub struct ScheduleId(/* private field */);
```

Identity of a durable schedule.

### `ScheduleId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for ScheduleId
impl Copy for ScheduleId
impl Debug for ScheduleId
impl Default for ScheduleId
impl<'de> Deserialize<'de> for ScheduleId
impl Display for ScheduleId
impl Eq for ScheduleId
impl FromStr for ScheduleId
impl Hash for ScheduleId
impl JsonSchema for ScheduleId
impl Ord for ScheduleId
impl PartialEq for ScheduleId
impl PartialOrd for ScheduleId
impl Serialize for ScheduleId
```

## enum `ScheduleRule`

`src/session.rs`

```rust
pub enum ScheduleRule {
    Once {
        at: crate::DateTime<crate::Utc>,
    },
    Cron {
        expr: String,
        timezone: Option<crate::Tz>,
    },
}
```

One instant or a recurring pattern interpreted in local time.

- `Once` — A single future instant.
  - `at` — Planned delivery time in UTC.
- `Cron` — A five-field cron expression (minute, hour, day, month, weekday).
  - `expr` — Pattern as provided by the caller.
  - `timezone` — IANA time zone; absent means follow the owner's configured zone.

Implements:

```rust
impl Clone for ScheduleRule
impl Debug for ScheduleRule
impl<'de> Deserialize<'de> for ScheduleRule
impl PartialEq for ScheduleRule
impl Serialize for ScheduleRule
```

## enum `ScheduleState`

`src/session.rs`

```rust
pub enum ScheduleState {
    Active,
    Done,
    Cancelled,
}
```

Schedule lifetime; firing and Inbox acceptance are atomic.

- `Active` — Future occurrences may be delivered.
- `Done` — The last occurrence has been delivered.
- `Cancelled` — Future occurrences have been cancelled.

Implements:

```rust
impl Clone for ScheduleState
impl Copy for ScheduleState
impl Debug for ScheduleState
impl<'de> Deserialize<'de> for ScheduleState
impl Eq for ScheduleState
impl PartialEq for ScheduleState
impl Serialize for ScheduleState
```

## struct `SchemaError`

`src/schema.rs`

```rust
pub struct SchemaError {
    /* private fields */
}
```

Why a declared schema is outside the canonical subset, and where, as a JSON Pointer.

Implements:

```rust
impl Clone for SchemaError
impl Debug for SchemaError
impl Display for SchemaError
impl Eq for SchemaError
impl Error for SchemaError
impl PartialEq for SchemaError
```

## struct `Seq`

`src/ids.rs`

```rust
pub struct Seq(pub u64);
```

Sequence assigned by a Session actor, starting at one.

Implements:

```rust
impl Clone for Seq
impl Copy for Seq
impl Debug for Seq
impl<'de> Deserialize<'de> for Seq
impl Eq for Seq
impl Hash for Seq
impl Ord for Seq
impl PartialEq for Seq
impl PartialOrd for Seq
impl Serialize for Seq
```

## struct `SessionId`

`src/ids.rs`

```rust
pub struct SessionId(/* private field */);
```

Durable identity of a conversation and its Log.

### `SessionId::new`

```rust
pub fn new() -> Self
```

Allocate a globally unique identity, independent of ordering.

Implements:

```rust
impl Clone for SessionId
impl Copy for SessionId
impl Debug for SessionId
impl Default for SessionId
impl<'de> Deserialize<'de> for SessionId
impl Display for SessionId
impl Eq for SessionId
impl FromStr for SessionId
impl Hash for SessionId
impl JsonSchema for SessionId
impl Ord for SessionId
impl PartialEq for SessionId
impl PartialOrd for SessionId
impl Serialize for SessionId
```

## struct `SessionRecord`

`src/session.rs`

```rust
pub struct SessionRecord {
    pub id: crate::SessionId,
    pub name: String,
    pub created_at: crate::DateTime<crate::Utc>,
    pub binding: Binding,
    pub profile: String,
}
```

Persistent Session identity, execution binding and selected profile.

- `id` — Identity joining the Session’s Inbox, Log and execution binding.
- `name` — Unique, human-readable name used by native commands.
- `created_at` — Creation time in UTC.
- `binding` — Node and generation authorized to execute this Session.
- `profile` — Name of the configuration selected at the next Round boundary.

Implements:

```rust
impl Clone for SessionRecord
impl Debug for SessionRecord
impl<'de> Deserialize<'de> for SessionRecord
impl PartialEq for SessionRecord
impl Serialize for SessionRecord
```

## struct `SessionRef`

`src/session.rs`

```rust
pub struct SessionRef {
    pub id: crate::SessionId,
    pub name: String,
}
```

A Session's stable identity with its resolved name for inspection.

- `id` — Identity used by the Session's durable records.
- `name` — Human-readable name resolved by the record owner.

Implements:

```rust
impl Clone for SessionRef
impl Debug for SessionRef
impl Eq for SessionRef
impl PartialEq for SessionRef
impl Serialize for SessionRef
```

## enum `Settlement`

`src/tool.rs`

```rust
pub enum Settlement {
    Ok,
    Failed {
        failure: Failure,
    },
    Unknown {
        failure: Failure,
    },
}
```

Durable execution outcome; result text is stored once in the Log entry's content or blob.

- `Ok` — The operation completed successfully.
- `Failed` — The operation failed with known effects.
  - `failure` — Classification and details of the failure.
- `Unknown` — Effects may already have occurred; never retry automatically.
  - `failure` — Classification and details of the failure.

Implements:

```rust
impl Clone for Settlement
impl Debug for Settlement
impl<'de> Deserialize<'de> for Settlement
impl From<&Outcome> for Settlement
impl PartialEq for Settlement
impl Serialize for Settlement
```

## struct `Source`

`src/plan.rs`

```rust
pub struct Source {
    pub id: String,
    pub hash: crate::ContentHash,
}
```

A source an inline message renders, named and hashed by the composer that wrote it.

- `id` — The composer's name for the source, such as `memory:<id>`.
- `hash` — Content hash of the source's original text, before formatting.

Implements:

```rust
impl Clone for Source
impl Debug for Source
impl<'de> Deserialize<'de> for Source
impl PartialEq for Source
impl Serialize for Source
```

## enum `StopReason`

`src/entry.rs`

```rust
pub enum StopReason {
    EndTurn,
    ToolCalls,
    MaxTokens,
    Other,
}
```

Provider-reported completion boundary.

- `EndTurn` — The provider finished its reply.
- `ToolCalls` — The provider requested tools.
- `MaxTokens` — The output limit was reached.
- `Other` — A provider-specific stopping condition.

Implements:

```rust
impl Clone for StopReason
impl Copy for StopReason
impl Debug for StopReason
impl<'de> Deserialize<'de> for StopReason
impl Eq for StopReason
impl PartialEq for StopReason
impl Serialize for StopReason
```

## struct `ToolCall`

`src/message.rs`

```rust
pub struct ToolCall {
    pub id: crate::CallId,
    pub provider_id: String,
    pub name: String,
    pub arguments: String,
}
```

A proposed invocation; arguments are normalized only when dispatched.

- `id` — Fresh identity assigned by the host adapter when accepting a Completion.
- `provider_id` — Original service call ID, empty when the service assigned none.
- `name` — Tool name selected by the model from the disclosed definitions.
- `arguments` — Raw provider arguments, parsed only at dispatch.

Implements:

```rust
impl Clone for ToolCall
impl Debug for ToolCall
impl<'de> Deserialize<'de> for ToolCall
impl PartialEq for ToolCall
impl Serialize for ToolCall
```

## struct `ToolResult`

`src/message.rs`

```rust
pub struct ToolResult {
    pub call: crate::CallId,
    pub provider_id: String,
    pub content: String,
    pub is_error: bool,
}
```

Model-facing settlement linked to the original provider call ID.

- `call` — Host identity of the proposed tool call.
- `provider_id` — Original service call ID, retained when returning tool results.
- `content` — Exact model-visible result text.
- `is_error` — Whether this is a failed or unknown tool outcome.

Implements:

```rust
impl Clone for ToolResult
impl Debug for ToolResult
impl<'de> Deserialize<'de> for ToolResult
impl PartialEq for ToolResult
impl Serialize for ToolResult
```

## struct `ToolSpec`

`src/tool.rs`

```rust
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: crate::InputSchema,
    pub effect: Effect,
}
```

A model-facing tool contract; its name is unique within a Round snapshot.

It is recorded only as a `DisclosedTool`.

- `name` — Name accepted in model tool proposals and matched by dispatch.
- `description` — Instructions explaining the capability to the model.
- `input_schema` — The argument object; dispatch checks every call against it before the tool runs.
- `effect` — Mutation semantics used when recovering an unsettled call.

### `ToolSpec::of`

```rust
pub fn of<A: JsonSchema>(name: impl Into<String>, description: impl Into<String>, effect: Effect) -> Result<Self, SchemaError>
```

Declare a tool whose arguments are `A`; the schema comes from `A` and its field documentation.

Implements:

```rust
impl Clone for ToolSpec
impl Debug for ToolSpec
impl From<&ToolSpec> for DisclosedTool
impl PartialEq for ToolSpec
```

## struct `Unviewable`

`src/image.rs`

```rust
pub struct Unviewable {
    pub what: String,
    pub reason: UnviewableReason,
}
```

An attachment the owner sent but this host could not make visible as an image.

- `what` — Protocol-provided description of the attachment.
- `reason` — Recorded reason it was not accepted as an image.

Implements:

```rust
impl Clone for Unviewable
impl Debug for Unviewable
impl<'de> Deserialize<'de> for Unviewable
impl Eq for Unviewable
impl PartialEq for Unviewable
impl Serialize for Unviewable
```

## enum `UnviewableReason`

`src/image.rs`

```rust
pub enum UnviewableReason {
    Unsupported,
    TooLarge,
    Unavailable(String),
}
```

Expected attachment failures remain part of their input Event.

- `Unsupported` — The attachment is not a supported image encoding.
- `TooLarge` — Its original bytes exceed the image limit.
- `Unavailable` — The service permanently refused retrieval, with its diagnostic context.

Implements:

```rust
impl Clone for UnviewableReason
impl Debug for UnviewableReason
impl<'de> Deserialize<'de> for UnviewableReason
impl Eq for UnviewableReason
impl PartialEq for UnviewableReason
impl Serialize for UnviewableReason
```

## struct `Usage`

`src/entry.rs`

```rust
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: Option<u64>,
}
```

Token counts reported by the provider.

- `input_tokens` — Total input tokens reported by the provider.
- `output_tokens` — Total output tokens reported by the provider.
- `cached_input_tokens` — Cached input tokens, if the provider reports them.

Implements:

```rust
impl Clone for Usage
impl Debug for Usage
impl<'de> Deserialize<'de> for Usage
impl PartialEq for Usage
impl Serialize for Usage
```

## fn `parse_arguments`

`src/tool.rs`

```rust
pub fn parse_arguments<A: DeserializeOwned>(arguments: serde_json::Map<String, serde_json::Value>) -> Result<A, Failure>
```

Parse a call's arguments into the tool's argument type.

Dispatch has already checked them against the declared schema, so this fails only on rules the
type enforces itself, such as the characters allowed in a plugin name.

## const `CONFIGURATION_FILE`

`src/intent.rs`

```rust
pub const CONFIGURATION_FILE: &str = "config.toml";
```

The owner's configuration file, relative to the Enco home.

## const `DEFAULT_PROFILE`

`src/session.rs`

```rust
pub const DEFAULT_PROFILE: &str = "default";
```

Profile selected when a Session is created; every node configuration must define it.

## const `PLUGINS_DIRECTORY`

`src/intent.rs`

```rust
pub const PLUGINS_DIRECTORY: &str = "plugins";
```

The directory holding one source directory per plugin, relative to the Enco home.

## const `REASON_PLACEHOLDER`

`src/registration.rs`

```rust
pub const REASON_PLACEHOLDER: &str = "<why>";
```

Reserved marker in suggested calls; it is never an acceptable registration reason.

# module `enco_core::code`

`src/tool.rs`

Stable failure codes shared with WIT plugins.

- const `code::CANCELLED` — Execution stopped in response to its cancellation token.
- const `code::COMPOSE_FAILED` — The composer could not prepare a request.
- const `code::CONFIG_INVALID` — A registered configuration table is invalid for the current host.
- const `code::CONFIG_REJECTED` — An intentional configuration registration failed validation or lost a commit race.
- const `code::CONTEXT_FAILED` — A context source could not read its authoritative content.
- const `code::CONTEXT_OVERFLOW` — The smallest valid request exceeds the configured window.
- const `code::CREDENTIAL_UNAVAILABLE` — The endpoint's credential variable is unset or empty, so the plugin was not called.
- const `code::INTERRUPTED` — The previous process stopped before settlement.
- const `code::NOT_DISPATCHED` — A proposed invocation never reached its tool.
- const `code::PLAN_INVALID` — The composer’s plan violates the execution contract.
- const `code::PLUGIN_CONTRACT` — The component returned data that violates its WIT interface contract.
- const `code::PLUGIN_LOAD` — A registered component could not be read or loaded by the runtime.
- const `code::PLUGIN_REJECTED` — An explicit deployment or rollback could not be accepted.
- const `code::PLUGIN_ROLLED_BACK` — The Round's bound generation has been marked failed.
- const `code::PLUGIN_TRAP` — The component trapped or could not be instantiated or invoked.
- const `code::PLUGIN_UNAVAILABLE` — No active generation exports the caller's required interface.
- const `code::PROFILE_UNKNOWN` — The Session refers to a profile absent from the current node configuration.
- const `code::PROVIDER_AUTH` — The service rejected the configured credentials.
- const `code::PROVIDER_BAD_REQUEST` — The adapter or service rejected the request.
- const `code::PROVIDER_BAD_RESPONSE` — The service's answer is unusable: it violates the protocol or fails the caller's check.
- const `code::PROVIDER_NETWORK` — The provider request failed in transport.
- const `code::PROVIDER_RATE_LIMITED` — The service asked the caller to slow down.
- const `code::PROVIDER_SERVER` — The service returned a server error.
- const `code::TIMEOUT` — The operation exceeded its deadline.
- const `code::TOOL_FAILED` — A dispatched tool returned a known failure.
- const `code::TOOL_INVALID_ARGUMENTS` — The tool cannot accept the supplied argument object.
- const `code::TOOL_UNAVAILABLE` — The proposed tool is absent from this Round’s disclosure or snapshot.

## const `code::CANCELLED`

`src/tool.rs`

```rust
pub const CANCELLED: &str = "cancelled";
```

Execution stopped in response to its cancellation token.

## const `code::COMPOSE_FAILED`

`src/tool.rs`

```rust
pub const COMPOSE_FAILED: &str = "compose.failed";
```

The composer could not prepare a request.

## const `code::CONFIG_INVALID`

`src/tool.rs`

```rust
pub const CONFIG_INVALID: &str = "config.invalid";
```

A registered configuration table is invalid for the current host.

## const `code::CONFIG_REJECTED`

`src/tool.rs`

```rust
pub const CONFIG_REJECTED: &str = "config.rejected";
```

An intentional configuration registration failed validation or lost a commit race.

## const `code::CONTEXT_FAILED`

`src/tool.rs`

```rust
pub const CONTEXT_FAILED: &str = "context.failed";
```

A context source could not read its authoritative content.

## const `code::CONTEXT_OVERFLOW`

`src/tool.rs`

```rust
pub const CONTEXT_OVERFLOW: &str = "context.overflow";
```

The smallest valid request exceeds the configured window.

## const `code::CREDENTIAL_UNAVAILABLE`

`src/tool.rs`

```rust
pub const CREDENTIAL_UNAVAILABLE: &str = "credential.unavailable";
```

The endpoint's credential variable is unset or empty, so the plugin was not called.

## const `code::INTERRUPTED`

`src/tool.rs`

```rust
pub const INTERRUPTED: &str = "interrupted";
```

The previous process stopped before settlement.

## const `code::NOT_DISPATCHED`

`src/tool.rs`

```rust
pub const NOT_DISPATCHED: &str = "not_dispatched";
```

A proposed invocation never reached its tool.

## const `code::PLAN_INVALID`

`src/tool.rs`

```rust
pub const PLAN_INVALID: &str = "plan.invalid";
```

The composer’s plan violates the execution contract.

## const `code::PLUGIN_CONTRACT`

`src/tool.rs`

```rust
pub const PLUGIN_CONTRACT: &str = "plugin.contract";
```

The component returned data that violates its WIT interface contract.

## const `code::PLUGIN_LOAD`

`src/tool.rs`

```rust
pub const PLUGIN_LOAD: &str = "plugin.load";
```

A registered component could not be read or loaded by the runtime.

## const `code::PLUGIN_REJECTED`

`src/tool.rs`

```rust
pub const PLUGIN_REJECTED: &str = "plugin.rejected";
```

An explicit deployment or rollback could not be accepted.

## const `code::PLUGIN_ROLLED_BACK`

`src/tool.rs`

```rust
pub const PLUGIN_ROLLED_BACK: &str = "plugin.rolled_back";
```

The Round's bound generation has been marked failed.

## const `code::PLUGIN_TRAP`

`src/tool.rs`

```rust
pub const PLUGIN_TRAP: &str = "plugin.trap";
```

The component trapped or could not be instantiated or invoked.

## const `code::PLUGIN_UNAVAILABLE`

`src/tool.rs`

```rust
pub const PLUGIN_UNAVAILABLE: &str = "plugin.unavailable";
```

No active generation exports the caller's required interface.

## const `code::PROFILE_UNKNOWN`

`src/tool.rs`

```rust
pub const PROFILE_UNKNOWN: &str = "profile.unknown";
```

The Session refers to a profile absent from the current node configuration.

## const `code::PROVIDER_AUTH`

`src/tool.rs`

```rust
pub const PROVIDER_AUTH: &str = "provider.auth";
```

The service rejected the configured credentials.

## const `code::PROVIDER_BAD_REQUEST`

`src/tool.rs`

```rust
pub const PROVIDER_BAD_REQUEST: &str = "provider.bad_request";
```

The adapter or service rejected the request.

## const `code::PROVIDER_BAD_RESPONSE`

`src/tool.rs`

```rust
pub const PROVIDER_BAD_RESPONSE: &str = "provider.bad_response";
```

The service's answer is unusable: it violates the protocol or fails the caller's check.

## const `code::PROVIDER_NETWORK`

`src/tool.rs`

```rust
pub const PROVIDER_NETWORK: &str = "provider.network";
```

The provider request failed in transport.

## const `code::PROVIDER_RATE_LIMITED`

`src/tool.rs`

```rust
pub const PROVIDER_RATE_LIMITED: &str = "provider.rate_limited";
```

The service asked the caller to slow down.

## const `code::PROVIDER_SERVER`

`src/tool.rs`

```rust
pub const PROVIDER_SERVER: &str = "provider.server";
```

The service returned a server error.

## const `code::TIMEOUT`

`src/tool.rs`

```rust
pub const TIMEOUT: &str = "timeout";
```

The operation exceeded its deadline.

## const `code::TOOL_FAILED`

`src/tool.rs`

```rust
pub const TOOL_FAILED: &str = "tool.failed";
```

A dispatched tool returned a known failure.

## const `code::TOOL_INVALID_ARGUMENTS`

`src/tool.rs`

```rust
pub const TOOL_INVALID_ARGUMENTS: &str = "tool.invalid_arguments";
```

The tool cannot accept the supplied argument object.

## const `code::TOOL_UNAVAILABLE`

`src/tool.rs`

```rust
pub const TOOL_UNAVAILABLE: &str = "tool.unavailable";
```

The proposed tool is absent from this Round’s disclosure or snapshot.
