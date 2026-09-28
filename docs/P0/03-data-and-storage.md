# 03 领域类型与存储

本文件定义 enco-core 中的全部领域类型，以及 Store 端口与它的 SQLite 实现。这些类型是 P0 的词汇表：新增或改动任何一个都需要先提问。

下面的 Rust 代码给出字段与语义；`derive` 省略不写，默认都有 `Debug, Clone, PartialEq, Serialize, Deserialize`，ID 类型另有 `Copy, Eq, Hash, Ord`。

## 1. enco-core

### 1.1 ID 与位置（`ids.rs`）

```rust
/// ULID newtype。每个 ID 类型一个，`new()` 生成新值，Display/FromStr 使用 ULID 字符串。
pub struct SessionId(Ulid);
pub struct EventId(Ulid);
pub struct RunId(Ulid);
pub struct RoundId(Ulid);
pub struct AttemptId(Ulid);
pub struct CallId(Ulid);
pub struct ScheduleId(Ulid);
pub struct MemoryId(Ulid);
pub struct NodeId(Ulid);

/// Binding 的代数。单节点时恒为 1。
pub struct Epoch(pub u64);
/// 同一 epoch 内的序号，从 1 开始，由 Session actor 分配。
pub struct Seq(pub u64);

/// Log 中的位置。按 (epoch, seq) 的字典序比较，这就是 Log 的全序。
pub struct LogPos { pub epoch: Epoch, pub seq: Seq }
```

`ids.rs` 中允许用一个 `macro_rules!` 生成九个 ULID newtype，这是本规格中唯一允许的宏。

### 1.2 内容哈希（`hash.rs`）

```rust
/// 字节内容的 blake3 哈希。Display 与 serde 均为 64 位小写十六进制。
pub struct ContentHash([u8; 32]);
impl ContentHash { pub fn of(bytes: &[u8]) -> Self; }
```

### 1.3 模型消息（`message.rs`）

与服务商无关的规范消息。这是发给 Provider 的唯一消息形态。

```rust
pub enum Role { System, User, Assistant, Tool }

pub struct Message { pub role: Role, pub parts: Vec<Part> }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    /// 服务商特有的字段（例如推理内容），由产生它的 Provider 原样取回；内核从不解释。
    Extension(Extension),
}

pub struct ToolCall {
    pub id: CallId,            // 内核身份，由 Provider 适配层在产生 Completion 时分配
    pub provider_id: String,   // 服务商返回的调用 ID，回传工具结果时使用
    pub name: String,          // 模型看到的工具名
    pub arguments: String,     // 模型给出的原始参数文本，不在此处解析
}

pub struct ToolResult {
    pub call: CallId,
    pub provider_id: String,
    pub content: String,       // 模型看到的结果文本
    pub is_error: bool,
}

pub struct Extension { pub provider: String, pub data: serde_json::Value }
```

不变式：`System` 与 `User` 消息只含 `Text`；`Tool` 消息恰好含一个 `ToolResult`；`Assistant` 消息可以含 `Text`、`ToolCall`、`Extension`。

提供 `Message::text(role, text)`、`Message::tool_calls()`、`Message::joined_text()` 三个便利方法，不要再加别的。

### 1.4 Event（`event.rs`）

Event 是送进 Inbox 的输入。

```rust
pub struct Event {
    pub id: EventId,               // 由提交方生成；重复提交同一 ID 是幂等的
    pub session: SessionId,
    pub source: EventSource,
    pub body: EventBody,
    pub received_at: DateTime<Utc>, // 仅用于展示，不参与排序
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventSource { Cli, Scheduler }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventBody {
    UserMessage { text: String },
    Reminder { schedule: ScheduleId, due_at: DateTime<Utc>, text: String },
}

impl Event {
    /// 事件的规范消息形态。全系统只在这里定义事件如何成为模型消息。
    pub fn canonical_message(&self) -> Message;
}
```

规范形态：`UserMessage` → `User` 消息，内容即 `text`；`Reminder` → `User` 消息，内容为 `"[reminder scheduled for {due_at}] {text}"`，其中 `due_at` 为 RFC 3339 UTC。

### 1.5 Log 条目（`entry.rs`）

Log 是 Session 的全部事实。十种条目构成完整的词汇，**不要增加**。

```rust
pub struct Entry { pub pos: LogPos, pub at: DateTime<Utc>, pub body: EntryBody }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryBody {
    /// Inbox 中的一个 Event 进入 Session 历史。同一 Commit 中消费对应的 Inbox 行。
    EventConsumed { event: Event },
    RunStarted { run: RunId },
    RoundStarted { run: RunId, round: RoundId, safe_mode: bool },
    /// 预写：在调用 Provider 之前提交。`plan` 是序列化后的 ContextPlan 在 blob 存储中的哈希。
    AttemptStarted {
        round: RoundId,
        attempt: AttemptId,
        purpose: AttemptPurpose,
        plan: ContentHash,
        composer: CodeRef,
        provider: CodeRef,
    },
    AttemptSettled { attempt: AttemptId, result: AttemptResult },
    /// 预写：在工具真正执行之前提交。没有真正分派的调用不写这一条。
    ToolCallStarted { round: RoundId, call: CallId, capability: CapabilityId, code: CodeRef, effect: Effect },
    /// 已结算的 Reply Attempt 中的每个工具调用恰好对应一条，无论是否真正分派。
    ToolCallSettled { call: CallId, outcome: Outcome, content: String, full: Option<ContentHash> },
    RoundEnded { round: RoundId, end: RoundEnd },
    RunEnded { run: RunId, end: RunEnd },
    /// 位置不大于 `upto` 的条目在 Transcript 中由 `summary` 代替。条目本身永不删除。
    Compacted { upto: LogPos, summary: String, attempt: AttemptId },
}

pub enum AttemptPurpose { Reply, Compaction }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttemptResult {
    Completed { message: Message, usage: Usage, stop: StopReason },
    Failed { failure: Failure },
}

pub struct Usage { pub input_tokens: u64, pub output_tokens: u64, pub cached_input_tokens: Option<u64> }

pub enum StopReason { EndTurn, ToolCalls, MaxTokens, Other }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoundEnd { Replied, ToolsSettled, Interrupted, Cancelled, Failed { failure: Failure } }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEnd { Completed, BudgetExhausted, Interrupted, Cancelled, Failed { failure: Failure } }
```

条目的**规范消息形态**只有三种，定义在 `entry.rs` 中，由 Transcript（04 §5）使用：

| 条目 | 规范消息 |
|---|---|
| `EventConsumed` | `event.canonical_message()` |
| `AttemptSettled`（`Completed`，对应的 Attempt 目的为 `Reply`） | `result.message` |
| `ToolCallSettled` | `Tool` 消息，含一个 `ToolResult { call, provider_id, content, is_error }`；`provider_id` 取自对应的 `ToolCall`，`is_error` 在结果不是 `Ok` 时为真 |

规范消息形态属于持久格式：已记录的计划通过它解析 Log 引用（§1.8），改变它就等于改变 Log 的格式。

### 1.6 工具（`tool.rs`）

```rust
/// 工具的自我描述。`name` 是模型看到的名字，在一个 Snapshot 中唯一。
pub struct ToolSpec { pub name: String, pub description: String, pub input_schema: serde_json::Value, pub effect: Effect }

/// 决定中断与取消后如何结算（04 §8）。
pub enum Effect { ReadOnly, Idempotent, SideEffect }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Ok { value: serde_json::Value },
    /// 确定没有产生外部效果，或者效果已知。
    Failed { failure: Failure },
    /// 可能已经执行。禁止自动重试。
    Unknown { failure: Failure },
}

pub struct Failure { pub code: String, pub message: String, pub retryable: bool }

/// 工具参数的唯一归一化位置。
pub enum Arguments { Object(serde_json::Map<String, serde_json::Value>), Invalid { raw: String } }
impl Arguments { pub fn parse(raw: &str) -> Self; }
```

`Arguments::parse` 的规则：先 trim；空串、`null`、`{}` 都视为空对象；内容是 JSON 字符串且解码后为对象的（被二次编码），解码一次；对象即对象；其余（数组、标量、截断的 JSON）为 `Invalid`，保留原文。

失败码集中定义在 `tool.rs` 的 `pub mod code` 中，只使用下表中的值：

| 码 | 含义 | 使用者 |
|---|---|---|
| `interrupted` | 进程崩溃导致中断 | 恢复 |
| `not_dispatched` | 模型请求了，但从未执行 | 恢复 |
| `cancelled` | 被主人取消 | 取消 |
| `timeout` | 超时 | 工具、Provider |
| `tool.unavailable` | 工具不存在或本轮未披露 | 分派 |
| `tool.invalid_arguments` | 参数无法归一化为对象，或不符合工具要求 | 分派、工具 |
| `tool.failed` | 工具执行失败（具体原因在 message 中） | 工具 |
| `plan.invalid` | ContextPlan 未通过校验 | 内核 |
| `compose.failed` | composer 返回错误 | 内核 |
| `context.failed` | ContextSource 返回错误 | 内核 |
| `context.overflow` | 压缩之后仍然放不下 | composer |
| `provider.network` `provider.auth` `provider.rate_limited` `provider.server` `provider.bad_request` `provider.bad_response` | Provider 的失败分类（07 §4.4） | Provider |

### 1.7 能力（`capability.rs`）

```rust
/// 带节点限定的能力 ID。Display 为 "name@node"。模型只看到 `name`。
pub struct CapabilityId { pub node: NodeId, pub name: String }

/// 记录"这一次到底是哪份代码在运行"。
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeRef {
    Native { name: String, version: String },  // version 为 crate 版本
    Wasm { artifact: ContentHash },            // 组件字节的哈希
}
```

### 1.8 上下文计划（`plan.rs`）

```rust
/// composer 的输出，也就是准备发出的请求（架构文档 §7.1）。按内容寻址存入 blob 存储。
pub struct ContextPlan {
    pub items: Vec<PlanItem>,          // 有序的请求消息
    pub tools: Vec<(CapabilityId, ToolSpec)>, // 披露集：模型本次能看到、也只能调用的工具，连同它看到的完整定义
    pub max_output_tokens: Option<u32>,
    pub omitted: Vec<Omission>,        // 被省略的来源及原因，用于诊断
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanItem {
    Message { message: Message },      // composer 生成的内联内容
    Log { pos: LogPos },               // 引用 Log 条目的规范消息，避免在每个计划中复制历史
}

pub struct Omission { pub source: String, pub reason: String }

/// ContextSource 的一次输出（架构文档 §7.1 的"Context 贡献"）。
pub struct Contribution {                 // Default：两者皆空
    pub candidates: Vec<Candidate>,       // 顺序即优先级
    pub omitted: Vec<Omission>,           // 来源自己未能提供的内容及原因
}

/// 一段候选内容。`id` 带来源前缀，例如 "workspace:AGENTS.md"、"memory:<MemoryId>"。
pub struct Candidate { pub id: String, pub kind: CandidateKind, pub text: String }
pub enum CandidateKind { Instruction, Memory }
```

**计划就是冻结的请求。** 模型看到的每一样东西，要么在计划中，要么在它引用的不可变 Log 条目中。Provider 请求只由计划与 Log 解析得到，不再读取 Snapshot 或任何上下文源；重试与事后查看都解析同一份记录（04 §6.3）。

架构文档 §7.1 中的稳定性标注与"上一份计划作为输入"在 P0 中不实现：P0 的 Provider 不使用它们，以后可以增量加入。

### 1.9 Session 与 Schedule（`session.rs`）

```rust
pub struct SessionRecord {
    pub id: SessionId,
    pub name: String,              // 人类可读，唯一
    pub created_at: DateTime<Utc>,
    pub binding: Binding,
    pub config: SessionConfig,
}

/// 单节点时恒为 (本节点, Epoch(1))。
pub struct Binding { pub node: NodeId, pub epoch: Epoch }

pub struct SessionConfig { pub requires_lifeline: bool }   // Default: true

pub struct Schedule {
    pub id: ScheduleId,
    pub session: SessionId,
    pub due_at: DateTime<Utc>,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub state: ScheduleState,
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleState { Pending, Fired { event: EventId }, Cancelled }
```

### 1.10 记忆（`memory.rs`）

```rust
/// 一条记忆：关于主人的一句自成一体的陈述。权威在 memory.db（06 §2）。
pub struct Memory {
    pub id: MemoryId,
    pub text: String,
    /// 置顶的记忆在每个 Round 优先参与预算，不依赖检索；超预算可以省略并记录原因。
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// 创建时为 1，每次修改加 1。
    pub rev: u64,
}
```

内核不使用这个类型；它放在 enco-core，是因为宿主与 CLI 协议共用它，并且 `MemoryId` 与其他 ID 使用同一个生成机制。

### 1.11 Token 估算（`lib.rs`）

```rust
/// 保守估算：UTF-8 字节数除以 3 向上取整。中文约为一字一 token，英文会偏高。
/// 实际用量以 Provider 回报的 Usage 为准。
pub fn estimate_tokens(text: &str) -> u32;
```

全系统只有这一个估算函数。

## 2. Store 端口（enco-kernel `ports/store.rs`）

Store 是内核唯一的持久化端口。每组方法都注明了它的**唯一写者**，这是架构文档 §3.6 规则一在存储层的体现。

```rust
#[async_trait]
pub trait Store: Send + Sync {
    // ---- 节点（写者：Kernel）
    async fn node(&self) -> Result<NodeRecord, StoreError>;
    async fn set_safe_mode(&self, enabled: bool) -> Result<(), StoreError>;

    // ---- Session（写者：Kernel）
    async fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError>;
    async fn session_by_name(&self, name: &str) -> Result<Option<SessionRecord>, StoreError>;
    /// 不存在则创建，binding 为 (本节点, Epoch(1))，config 为默认值。`created_at` 由内核从 Clock 取得。
    async fn ensure_session(&self, name: &str, created_at: DateTime<Utc>) -> Result<SessionRecord, StoreError>;

    // ---- Log（写者：该 Session 的 actor）
    async fn log(&self, session: SessionId, after: Option<LogPos>) -> Result<Vec<Entry>, StoreError>;
    async fn commit(&self, session: SessionId, commit: Commit) -> Result<(), StoreError>;

    // ---- Inbox（任何人都可以投递；只有该 Session 的 actor 通过 commit 消费）
    async fn accept(&self, event: &Event) -> Result<Accepted, StoreError>;
    async fn pending(&self, session: SessionId) -> Result<Vec<Event>, StoreError>;

    // ---- Schedule（写者：Scheduler actor）
    async fn insert_schedule(&self, schedule: &Schedule) -> Result<(), StoreError>;
    async fn cancel_schedule(&self, id: ScheduleId) -> Result<bool, StoreError>;
    async fn schedules(&self, state: Option<ScheduleStateKind>) -> Result<Vec<Schedule>, StoreError>;
    /// 在一个事务中把 Schedule 从 Pending 改为 Fired，并把 `event` 投递进 Inbox。
    async fn fire_schedule(&self, id: ScheduleId, event: &Event) -> Result<(), StoreError>;

    // ---- Blob（内容寻址，写入幂等）
    async fn put_blob(&self, bytes: &[u8]) -> Result<ContentHash, StoreError>;
    async fn get_blob(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError>;
    /// blob 在磁盘上的路径。长结果的完整内容由此交给模型用 fs_read 读取。
    fn blob_path(&self, hash: &ContentHash) -> PathBuf;
}

pub struct NodeRecord { pub id: NodeId, pub safe_mode: bool }

/// 一次原子提交：若干 Log 条目，以及它们消费的 Inbox 行。
pub struct Commit { pub entries: Vec<Entry>, pub consumed: Vec<EventId> }

pub enum Accepted { New, Duplicate }

pub enum ScheduleStateKind { Pending, Fired, Cancelled }

#[derive(thiserror::Error)]
pub enum StoreError {
    #[error("session {session} is fenced: binding is {binding:?}, commit epoch is {epoch:?}")]
    Fenced { session: SessionId, binding: Binding, epoch: Epoch },
    #[error("session {session}: expected next position {expected:?}, got {got:?}")]
    OutOfOrder { session: SessionId, expected: LogPos, got: LogPos },
    #[error("inbox: {0}")]
    Inbox(String),
    #[error("schedule {0} is not pending")]
    ScheduleNotPending(ScheduleId),
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    #[error("blob {0} is corrupt or missing")]
    Blob(ContentHash),
    #[error("database was created by a newer Enco (schema version {0})")]
    NewerSchema(u32),
    #[error("storage: {0}")]
    Backend(String),
    #[error("commit contains no entries")]
    EmptyCommit,
}
```

### 2.1 Commit 的前置条件

在同一个事务中按顺序检查，任何一条失败都整体回滚：

1. `entries` 非空，否则返回 `EmptyCommit`。
2. Session 存在，`binding.node` 等于本节点，`binding.epoch` 等于 `entries[0].pos.epoch`，否则返回 `Fenced`。
3. `entries[0].pos` 等于该 Session 在此 epoch 下的下一个位置（已有最大 seq + 1，没有则为 1），并且后续条目的位置连续，否则返回 `OutOfOrder`。
4. `consumed` 中的每个 EventId：Inbox 行存在、属于该 Session、尚未被消费，并且本次 Commit 中有一条对应的 `EventConsumed`；该行的 `consumed_epoch/consumed_seq` 置为那条条目的位置。否则返回 `Inbox`。

位置由 Session actor 分配，Store 只检查。这是"带前置条件的写入"：它让"只有归属者在写"成为可以验证的事实。

### 2.2 Inbox 的语义

Inbox 是一个邮箱：任何人都可以投递（`accept`），只有归属者读取并消费。

- `accept` 是 `INSERT OR IGNORE`，以 EventId 去重，返回 `New` 或 `Duplicate`。重复投递不是错误，这让客户端可以安全地重发。
- 投递的顺序由自增的 `order_no` 决定，而不是 `received_at`。
- 消费只能经由 `commit`，与 `EventConsumed` 条目在同一事务中完成。

## 3. SQLite 实现（enco-host `store/`）

### 3.1 连接

- 数据库文件：`$ENCO_HOME/enco.db`。
- 一个 `rusqlite::Connection`，放在 `std::sync::Mutex` 中；每个方法在 `tokio::task::spawn_blocking` 中执行。不要引入连接池。
- 打开时执行：`PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;`
- 写事务使用 `BEGIN IMMEDIATE`。

### 3.2 模式（版本 1）

```sql
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
-- 键：schema_version = '1'，node_id = <ULID>，safe_mode = '0' | '1'

CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL UNIQUE,
  created_at    TEXT NOT NULL,
  binding_node  TEXT NOT NULL,
  binding_epoch INTEGER NOT NULL,
  config        TEXT NOT NULL            -- SessionConfig 的 JSON
) STRICT;

CREATE TABLE log (
  session_id TEXT NOT NULL REFERENCES sessions(id),
  epoch      INTEGER NOT NULL,
  seq        INTEGER NOT NULL,
  at         TEXT NOT NULL,
  body       TEXT NOT NULL,              -- EntryBody 的 JSON
  kind       TEXT GENERATED ALWAYS AS (json_extract(body, '$.kind')) VIRTUAL,
  PRIMARY KEY (session_id, epoch, seq)
) STRICT;

CREATE TABLE inbox (
  order_no       INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id       TEXT NOT NULL UNIQUE,
  session_id     TEXT NOT NULL REFERENCES sessions(id),
  event          TEXT NOT NULL,          -- Event 的 JSON
  consumed_epoch INTEGER,
  consumed_seq   INTEGER
) STRICT;
CREATE INDEX inbox_pending ON inbox(session_id, order_no) WHERE consumed_seq IS NULL;

CREATE TABLE schedules (
  id             TEXT PRIMARY KEY,
  session_id     TEXT NOT NULL REFERENCES sessions(id),
  due_at         TEXT NOT NULL,
  message        TEXT NOT NULL,
  created_at     TEXT NOT NULL,
  state          TEXT NOT NULL CHECK (state IN ('pending', 'fired', 'cancelled')),
  fired_event_id TEXT
) STRICT;
CREATE INDEX schedules_pending ON schedules(due_at) WHERE state = 'pending';
```

`kind` 是生成列，只用于查询与调试，事实来源仍然是 `body`。

### 3.3 版本

- 新库：在一个事务中建表，写入 `schema_version = '1'`，生成并写入 `node_id`，`safe_mode = '0'`。
- 已有库：`schema_version` 为 1 时直接使用；大于 1 时返回 `NewerSchema` 并拒绝启动。P0 不写迁移代码。

### 3.4 Blob 存储（`blobs.rs`）

- 路径：`$ENCO_HOME/blobs/<哈希前两位>/<完整哈希>`。
- 写入：已存在则直接返回；否则写到同目录的 `<哈希>.tmp.<ULID>`，`sync_all` 之后 `rename`。
- 读取：读出后重新计算哈希，不一致则返回 `Blob` 错误。
- 孤立的 blob（写入了但对应的 Commit 没有成功）是无害的，P0 不做回收。
