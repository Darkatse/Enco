# 04 内核（enco-kernel）

内核只负责四件事：**记录**（Log）、**绑定**（这一次用哪份代码）、**校验**（计划与工具调用）、**调度**（Session actor、Run/Round、Scheduler）。它不包含 IO 实现、上下文提示词或排版，也不按具体工具、Provider 或服务身份写分派逻辑（01 §2）。内核自己的 Schedule 命令经普通 Tool 端口接入（§11）。绑定的对象是代际，代际的归属者是注册表（11）；操作注册表的工具要读文件，所以由宿主提供（05 §2.6）。

内核也不知道记忆：记忆是宿主经 Tool 与 ContextSource 两个端口接入的能力（06）。

## 1. 架构形态

端口与适配器（hexagonal architecture）：内核通过端口与外界交互，适配器由宿主与 Wasm 层实现，在组合根一次性装配。

```text
            ┌──────────────────── enco-kernel ────────────────────┐
 CLI/守护进程 │  Kernel ── Session actor ── Run/Round ──┐            │
 ──submit──▶ │     │          (每 Session 一个)         │            │
             │  Scheduler actor      Registry（代际的归属者，11）   │
             │     ports: Store · Runtime · Provider · Embedding · Decision · Composer · ContextSource · Tool · Clock
             └─────────┬──────────┬───────────┬──────────────┬────┘
                  enco-host    enco-wasm    enco-host     enco-host / 内核内置
                  (SQLite)   (插件运行时)  (composer、上下文、fs/shell、记忆)
```

## 2. 端口

端口都位于真实的边界上（IO、插件、策略、时间）。Store 的完整定义见 03 §2。

插件一侧的端口如下：`Runtime` 把制品变成 `Loaded`，其中的适配器对应 WIT 的接口（07 §1）。

```rust
// ports/runtime.rs
#[async_trait]
pub trait Runtime: Send + Sync {
    /// 编译制品，实例化一次并调用 describe(config)，读出它导出的接口。
    /// 失败说明制品或配置不可用：不是组件、导入不满足、describe 返回错误。
    async fn load(&self, artifact: &[u8], config: &serde_json::Value) -> Result<Loaded, LoadError>;
}

/// 一个已加载的代际能提供的东西。没有导出的接口就是 None。
pub struct Loaded {
    pub summary: String,                       // describe 返回的一句话
    pub lifecycle: Arc<dyn Lifecycle>,
    pub completion: Option<Arc<dyn Provider>>,
    pub embedding: Option<Arc<dyn Embedding>>,
    pub decision: Option<Arc<dyn Decision>>,
}

#[async_trait]
pub trait Lifecycle: Send + Sync {
    /// 自检，不联网。失败即部署被拒绝（11 §7）。
    async fn probe(&self) -> Result<(), Failure>;
}

#[derive(thiserror::Error)]
#[error("{0}")]
pub struct LoadError(pub String);
```

```rust
// ports/provider.rs
#[async_trait]
pub trait Provider: Send + Sync {
    /// 一次非流式补全。调用参数每次传入；密钥单独传，不属于参数（12 §3）。
    /// 超时由实现负责，以 Failure { code: "timeout", retryable: true } 返回。
    async fn complete(&self, settings: &ProviderSettings, api_key: Option<&str>, request: ProviderRequest) -> Result<Completion, Failure>;
}

pub struct ProviderRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_output_tokens: Option<u32>,
}

/// `message.role` 为 Assistant；其中每个 ToolCall 的 `id` 已由实现分配为新的 CallId。
pub struct Completion { pub message: Message, pub usage: Usage, pub stop: StopReason }
```

```rust
// ports/embedding.rs
#[async_trait]
pub trait Embedding: Send + Sync {
    /// 把若干段文本转换为向量，结果与输入一一对应。内核自己不调用它，它供宿主使用（06 §3.3）。
    async fn embed(&self, settings: &ProviderSettings, api_key: Option<&str>, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure>;
}
```

```rust
// ports/decision.rs
#[async_trait]
pub trait Decision: Send + Sync {
    /// 对同一份 state 独立回答每个问题，答案与问题一一对应。内核自己不调用它，它供宿主使用（06 §5.1）。
    async fn decide(&self, settings: &ProviderSettings, api_key: Option<&str>, state: String, questions: Vec<Question>) -> Result<Vec<Answer>, Failure>;
}

pub struct Question { pub instructions: String, pub kind: QuestionKind }
pub enum QuestionKind { Predicate, Choice(Vec<Label>), Score(Vec<Label>) }   // Score 的等级由低到高
pub struct Label { pub name: String, pub description: Option<String> }

/// Choice 与 Score 的概率按 label 顺序排列，和为 1（07 §3.3）。
pub enum Answer { Predicate(f64), Choice(Vec<f64>), Score(Vec<f64>), Refused }
```

代际由谁记录：适配器不再自报 `code()`。调用方从导出表拿到 `Export { generation, adapter }`（11 §4.2），把 `generation` 写进 Log。

```rust
// ports/composer.rs
pub trait Composer: Send + Sync {
    fn code(&self) -> CodeRef;
    /// 纯函数：同样的输入得到同样的输出。不做 IO，不读时钟，不用随机数，因此是同步的。
    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError>;
}

pub struct ComposeInput {
    pub now: DateTime<Utc>,                // 本 Round 读取的时刻（Clock）
    pub timezone: Tz,                      // 主人的时区（KernelConfig），用于把时刻换算为当地时间
    pub session: SessionRecord,
    pub profile: Profile,                  // 本 Round 解析出的 profile（12）：两个用途的预算与 requires_lifeline
    pub transcript: Transcript,
    pub previous_plan: Option<ContextPlan>, // 本 Session 最近一次 Reply Attempt 的计划，由 composer 决定是否沿用（05 §4.1）
    pub context: Contribution,             // 各 ContextSource 的贡献按顺序合并，安全模式下不调用来源；上一份计划读不到时，内核也在 omitted 中记一条
    pub tools: Vec<(CapabilityId, ToolSpec)>, // 本轮可披露的工具；安全模式下只有救生集
    pub safe_mode: bool,
    pub compactions_left: u32,             // 本 Round 还允许几次压缩；为 0 时只能返回 Plan 或失败
}

pub struct Budget { pub context_tokens: u32, pub max_output_tokens: u32 }

/// Log 的模型视图，由内核投影（§5）。
pub struct Transcript {
    pub summary: Option<String>,        // 最近一次 Compacted 的摘要
    pub items: Vec<TranscriptItem>,     // 该次压缩之后、具有规范消息形态的条目，按 Log 位置递增
    pub round_ends: Vec<LogPos>,        // 该次压缩之后的 RoundEnded 位置，按 Log 位置递增，也就是合法的压缩边界
    pub run_ends: Vec<(LogPos, RunEnd)>, // 该次压缩之后的 RunEnded 事实，按 Log 位置递增；composer 决定如何叙述
}

/// `generation` 只有 Assistant 消息有：产生它的 Attempt 所用的 Provider 代际，扩展字段按它回放（§6.5）。
/// `event` 只有来自 Inbox 的输入有：所消费的 Event，composer 据它叙述到达时间与来源等处境（05 §4.3）。
pub struct TranscriptItem {
    pub pos: LogPos,
    pub message: Message,
    pub generation: Option<GenerationId>,
    pub event: Option<Event>,
}

pub enum Composition {
    Plan(ContextPlan),
    /// 请求先压缩：`upto` 必须属于 `round_ends`，`plan` 是生成摘要的请求（不带工具）。`compactions_left` 为 0 时不能返回它。
    Compact { upto: LogPos, plan: ContextPlan },
}

#[derive(thiserror::Error)]
pub enum ComposeError {
    #[error("context overflow: need {needed} tokens, window is {window}")]
    ContextOverflow { needed: u32, window: u32 },
    #[error("{0}")]
    Invalid(String),
}
```

`ComposeInput.profile` 带着整份 `Profile`，其中包括 `Endpoint.api_key`。P1 的 composer 是原生代码，输入既不序列化也不记录，所以可以接受；composer 成为插件、输入要跨越 WIT 之前，先把密钥从 `Profile` 中分离出去。

```rust
// ports/context.rs
#[async_trait]
pub trait ContextSource: Send + Sync {
    /// 为本 Round 提供候选内容。安全模式下内核不调用。
    /// `query.cancel` 触发后，必须停止等待，并在已经开始的本地写入结束之后才返回（与 Tool 相同，§9）。
    async fn contribute(&self, query: &ContextQuery) -> Result<Contribution, ContextError>;
}

pub struct ContextQuery {
    pub session: SessionRecord,
    /// 本 Session 最近一条 `EventConsumed` 中的 Event：主人最新的输入，或最新触发的提醒。
    pub latest_event: Option<Event>,
    pub new_input: bool,               // 本 Round 接纳了新 Event：上一个 RoundEnded 之后有 EventConsumed
    /// 本 Run 取消令牌的子令牌。
    pub cancel: CancellationToken,
}

#[derive(thiserror::Error)]
#[error("context source: {0}")]
pub struct ContextError(pub String);
```

```rust
// ports/tool.rs
#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn code(&self) -> CodeRef;
    /// `ctx.cancel` 触发后，必须停止已经开始的工作，并在停止之后才返回。
    /// `ctx.result_budget` 是结果可以内联的字节数；能分页的工具应在预算内按自己的单位停下（§6.6）。
    async fn call(&self, ctx: CallContext, args: serde_json::Map<String, serde_json::Value>) -> Outcome;
}

pub struct CallContext { pub session: SessionId, pub call: CallId, pub cancel: CancellationToken, pub result_budget: usize }
```

```rust
// ports/clock.rs
pub trait Clock: Send + Sync {
    /// 当前 UTC 时刻；不读取系统时区，当地时间按主人的时区换算（架构文档 §3.7）。
    fn now(&self) -> DateTime<Utc>;
}
```


## 3. Kernel（`kernel.rs`）

```rust
pub struct KernelDeps {
    pub store: Arc<dyn Store>,
    pub registry: Arc<Registry>,     // 代际的归属者，由组合根先于 Kernel 打开（11 §4）
    pub profiles: BTreeMap<String, Profile>,   // 来自配置（12 §2）；必须含 "default"
    pub composer: Arc<dyn Composer>,
    pub context: Vec<Arc<dyn ContextSource>>,
    pub tools: Vec<Arc<dyn Tool>>,   // 宿主提供的工具；内核再加入自己的内置工具（§11）
    pub lifeline: Vec<String>,       // 组成救生集的工具名
    pub clock: Arc<dyn Clock>,
}

/// 字段不公开，只能经 `KernelConfig::new` 构造。
pub struct KernelConfig {
    max_rounds_per_run: u32,
    timezone: Tz,                 // 主人的时区（08 §3）：composer 的时间显示与跟随型周期定时都用它
}

impl KernelConfig {
    /// Round 数必须为正，否则返回 `KernelError::Config`。预算随 profile 走，由配置读取时检查（12 §2）。
    pub fn new(max_rounds_per_run: u32, timezone: Tz) -> Result<Self, KernelError>;
}

impl Kernel {
    /// 构造 Snapshot，启动 Scheduler，为每个已有 Session 启动 actor（actor 启动时自行恢复）。
    /// `profiles` 中没有 "default" 时返回 `KernelError::UnknownProfile`。
    pub async fn start(deps: KernelDeps, config: KernelConfig) -> Result<Kernel, KernelError>;
    /// 不存在则创建（profile 为 "default"），并确保它的 actor 已经启动。
    pub async fn open_session(&self, name: &str) -> Result<SessionRecord, KernelError>;
    /// 改变 Session 的 profile，下一个 Round 生效。名字不在 `profiles` 中返回 `UnknownProfile`。
    pub async fn set_profile(&self, session: SessionId, profile: &str) -> Result<(), KernelError>;
    /// 把一次 Attempt 记录的请求解析出来（§14）。`attempt` 为空时取该 Session 最近的一次。
    pub async fn inspect(&self, session: SessionId, attempt: Option<AttemptId>) -> Result<Inspection, KernelError>;
    pub fn registry(&self) -> &Arc<Registry>;
    /// 外部输入的唯一入口：经 `store.accept` 在一个事务中投递 `events` 并写入连接状态，然后唤醒这些 Event 的 Session。
    /// Event 的 Session 必须已经存在（`open_session`）。`events` 可以为空，此时只写连接状态。
    pub async fn accept(&self, events: &[Event], connection: Option<&ConnectionWrite>) -> Result<Vec<Accepted>, KernelError>;
    /// CLI 的便捷形式：构造 `Event { source: Cli, body: UserMessage, received_at: clock.now() }` 后调用 `accept`。
    /// 重复的 `event_id` 返回 Duplicate。
    pub async fn submit(&self, session: SessionId, event_id: EventId, text: String) -> Result<Accepted, KernelError>;
    /// 读取一个连接的状态（10 §2）。
    pub async fn connection(&self, key: &str) -> Result<Option<serde_json::Value>, KernelError>;
    pub async fn delivery_failures(&self, key: &str) -> Result<Vec<DeliverySettlement>, KernelError>;
    /// 订阅该 Session 之后提交的条目（进程内实时流，不是事实来源）。
    pub fn subscribe(&self, session: SessionId) -> Result<broadcast::Receiver<Entry>, KernelError>;
    /// 取消正在进行的 Run。返回是否确实有 Run 被取消。
    pub fn cancel(&self, session: SessionId) -> Result<bool, KernelError>;
    pub async fn set_safe_mode(&self, enabled: bool) -> Result<(), KernelError>;
    pub fn schedules(&self) -> Schedules;
    pub async fn status(&self) -> Result<Status, KernelError>;
    pub async fn sessions(&self) -> Result<Vec<SessionRecord>, KernelError>;
    pub async fn log(&self, session: SessionId, after: Option<LogPos>) -> Result<Vec<Entry>, KernelError>;
    /// 取消所有 Run，等待所有 actor 与 Scheduler 退出（发出信号 + 等待静止）。
    pub async fn shutdown(&self) -> Result<(), KernelError>;
}

pub struct Status {
    pub node: NodeId,
    pub safe_mode: bool,
    pub composer: CodeRef,
    pub profiles: Vec<String>,            // 配置中的 profile 名字
    pub sessions: Vec<SessionStatus>,
    pub scheduler_stopped: Option<String>, // Scheduler 因错误停止的原因，与 Session 的 stopped 同义
}

pub struct SessionStatus { pub session: SessionRecord, pub running: bool, pub stopped: Option<String> }
```

Provider 不再出现在 `Status` 里：它随 profile 与代际变化，看 `enco plugin status` 与每次 Attempt 的记录。

Scheduler 触发提醒时走 `store.fire_schedule`，它把 Schedule 的状态变化与 Inbox 投递放在同一个事务里，是同一种提交的内部形式。

守护进程与内置工具调用的是同一组方法：主人执行 `enco schedules --cancel` 与 Agent 调用 `schedule_cancel`，走的是同一个 `Schedules::cancel`；`enco plugin deploy` 与 `plugin_deploy` 走的是同一个 `Registry::deploy`。

## 4. Snapshot 与 Session actor

### 4.1 Snapshot（`snapshot.rs`）

`Kernel::start` 构造一次，之后不变：它只含原生的部分。插件的导出不在这里，而在注册表发布的导出表里，按调用解析（11 §4.2）。Round 钉住导出表要到 P2 有了工具插件才需要。

```rust
pub(crate) struct Snapshot {
    pub composer: Arc<dyn Composer>,
    pub context: Vec<Arc<dyn ContextSource>>,
    pub tools: Vec<SnapshotTool>,        // 宿主工具 + 内置工具
    pub lifeline: Vec<CapabilityId>,
}

pub(crate) struct SnapshotTool { pub id: CapabilityId, pub spec: ToolSpec, pub code: CodeRef, pub tool: Arc<dyn Tool> }
```

构造时检查：工具名唯一；`lifeline` 中的每个名字都存在。不满足返回 `KernelError::Config`。每个 Round 开始时克隆一次 `Arc<Snapshot>`，这一轮就钉住了这份能力。

### 4.2 Session actor（`session.rs`）

每个 Session 一个 tokio task，是该 Session Log 的唯一写者。

```rust
pub(crate) struct SessionHandle {
    pub id: SessionId,
    pub wake: Notify,                              // notify_one 会保留许可，不会丢失唤醒
    pub entries: broadcast::Sender<Entry>,
    pub run_cancel: Mutex<Option<CancellationToken>>, // 正在进行的 Run 的取消令牌
    pub stopped: Mutex<Option<String>>,             // actor 因错误退出时的原因
    pub task: Mutex<Option<JoinHandle<()>>>,
}
```

Kernel 持有 `Arc<Mutex<HashMap<SessionId, Arc<SessionHandle>>>>`。actor 的状态包括 `SessionRecord`、该 Session 全部条目的内存副本（启动时加载，提交后追加）、下一个序号 `next: u64`；位置由 Session 的 binding epoch 与该序号组成。

actor 生命周期：

```text
start:
    entries = store.log(session, None)
    bodies = recovery::recover(&entries)          // §8，纯函数
    if !bodies.is_empty(): commit(bodies, [])
loop:
    pending = store.pending(session)
    if pending.is_empty():
        select { wake.notified() => continue, shutdown.cancelled() => return }
    run()                                         // §6
```

`SessionRecord` 中只有 `profile` 会在运行中改变（`Kernel::set_profile`）。actor 在每个 Round 开始时用 `store.session(id)` 重读一次记录，所以改动在下一个 Round 生效，不需要给 actor 发消息。

**提交辅助函数** `commit(bodies, consumed)`：从 `next` 开始依次分配位置，`at = clock.now()`，调用 `store.commit`；成功后追加到内存副本，并把每个条目发送到 broadcast（没有订阅者时忽略发送错误）。

**错误处理**：`store.commit` 返回的任何错误都说明前提已被破坏（被 fence、位置错乱或存储故障）。actor 记录错误、写入 `stopped`，然后退出，任务返回 `()`。后续命令返回带原因的停止错误；shutdown 只等待静止并报告任务异常退出，不重复报告已记录的业务故障。Scheduler 遵守同一规则（§10）。

## 5. Transcript 投影（`transcript.rs`）

纯函数：`pub(crate) fn project(entries: &[Entry]) -> Transcript`。

1. 找到最近一条 `Compacted`。位置不大于它的 `upto` 的条目被隐藏，`summary` 取它的摘要；没有则 `summary = None`、不隐藏任何条目。
2. 遍历**全部**条目，记录每个 Attempt 的目的与 Provider 代际（来自 `AttemptStarted`）和每个 CallId 的 `provider_id`（来自已完成的 Reply 消息中的 `ToolCall`）。
3. 对未被隐藏的条目，按 Log 顺序：
   - 具有规范消息形态的条目（03 §1.5 的表）生成 `TranscriptItem`；目的为 `Compaction` 的 Attempt 不生成。`AttemptSettled` 生成的项带上该 Attempt 的代际，`EventConsumed` 生成的项带上该 Event，其余字段为空。
   - `RoundEnded` 的位置加入 `round_ends`。
   - `RunEnded` 的位置与结局加入 `run_ends`，不生成规范消息。

压缩边界总是 `RoundEnded`，所以工具调用与它的结果不会被拆开。

## 6. Run 与 Round（`run.rs`、`attempt.rs`、`dispatch.rs`、`plan.rs`）

### 6.1 Run

```text
run():
    run_id = RunId::new()
    token = shutdown.child_token(); handle.run_cancel = Some(token)
    prefix = [RunStarted { run: run_id }]         // 与第一个 Round 的开始一起提交
    for _ in 1..=max_rounds_per_run:
        end = round(run_id, token, take(prefix))
        if let Some(run_end) = end.terminal_run_end(): end_run(run_end); return   // Cancelled、Failed、Interrupted
        if end == Replied && store.pending(session).is_empty(): end_run(Completed); return
        // ToolsSettled，或 Replied 之后 Inbox 中又有新消息：继续下一个 Round
    end_run(BudgetExhausted)
    handle.run_cancel = None
```

`Interrupted` 只由恢复流程写入，Run 中不会产生。终止性的 Round 结局到 Run 结局的映射只有一处，即 enco-core 的 `RoundEnd::terminal_run_end`，与恢复流程写入的配对相同。

### 6.2 Round

```text
round(run, token, prefix) -> RoundEnd:
    round_id = RoundId::new()
    session = store.session(id)                    // 重读，profile 可能已改（§4.2）
    snapshot = self.deps.snapshot.clone()          // 本轮钉住
    safe_mode = store.node().safe_mode             // 安全模式在 Round 边界生效
    pending = store.pending(session)
    commit(prefix ++ [EventConsumed { e } for e in pending] ++ [RoundStarted { run, round_id, safe_mode }],
           consumed = pending 的 id)
    profile = profiles[session.profile]，没有 → end_round(Failed(profile.unknown，错误中带 profile 名称))

    planned = compose(snapshot, profile, round_id, safe_mode, token)?            // §6.3；计划携带本轮 composer 的 CodeRef
    completion = attempt(profile.reply, round_id, safe_mode, planned, token)?   // §6.4；取消或失败则 end_round
    calls = completion.message.tool_calls()
    if calls.is_empty(): return end_round(Replied)
    for call in calls: dispatch(snapshot, round_id, planned.plan, call, token)   // §6.6：是否开始由 dispatch 决定
    return end_round(if token.is_cancelled() { Cancelled } else { ToolsSettled })
```

`end_round(end)` 提交 `RoundEnded { round, end }` 并返回 `end`。

profile 找不到是配置问题，不是模型问题，所以 Round 失败、Run 失败，composer 从 `Transcript.run_ends` 得到事实并在下一条输入的说明中呈现（05 §4.3），主人从 `enco status` 或渠道的终止通知看到（12 §3）。

### 6.3 组装与压缩

组装分两个时刻。**组装时**读取当前状态：时间与上下文贡献每个 Round 读取一次，压缩只改变 Transcript，重新组装时复用它们。`new_input` 从已提交的 Log 得到：上一个 `RoundEnded` 之后存在 `EventConsumed` 时为真；内核始终提供 `latest_event`，由各来源决定是否利用新输入。计划、解析后的请求、用途与实际生成计划的 composer 标识共同构成 `PlannedAttempt`。**记录后**只解析记录：Attempt 与重试使用这份结果（§6.4），不再重新读取 composer 或 Snapshot 中的工具定义。

```text
compose(snapshot, profile, round, safe_mode, token):
    now = clock.now()                                          // 每个 Round 读取一次；当地时间由 composer 按主人的时区换算
    context = if safe_mode { Contribution::default() }
              else { 以 ContextQuery { session, latest_event, new_input, cancel: token.child_token() } 依次调用每个 ContextSource，
                     按顺序合并 candidates 与 omitted }
    if token.is_cancelled(): return Ended(Cancelled)          // 无论来源返回了什么（§9）
    任何一个来源出错 → Failed(context.failed)
    input = ComposeInput {
        now, timezone: config.timezone, session, profile, transcript: transcript::project(&entries),
        previous_plan: 最近一条目的为 Reply 的 AttemptStarted 的计划（从 blob 读取）,
        context,
        tools: if safe_mode { 救生集 } else { 全部 },
        safe_mode,
        compactions_left: MAX_COMPACTIONS_PER_ROUND,
    }
    loop:
        match composer.compose(&input):
            Err(e)                    => Failed(e 转为 Failure：ContextOverflow → context.overflow，其余 → compose.failed)
            Ok(Plan(plan))            => plan::validate(&plan, &input, Reply, &snapshot.lifeline)?            // 失败 → Failed(plan.invalid)
                                         return PlannedAttempt { kind: Reply, composer: snapshot.composer.code(), plan,
                                                  request: plan::resolve(&plan, &input.transcript, exports, target(Reply))? }
            Ok(Compact { upto, plan }) =>
                compactions_left 为 0 时 Failed(compose.failed，说明本 Round 压缩次数已用尽)
                upto 必须属于 input.transcript.round_ends，否则 Failed(plan.invalid)
                plan::validate(&plan, &input, Compaction, &snapshot.lifeline)?
                request = plan::resolve(&plan, &input.transcript, exports, target(Compaction))?
                planned = PlannedAttempt { kind: Compaction(upto), composer: snapshot.composer.code(), plan, request }
                attempt(profile.compaction, round, safe_mode, planned, token)?   // 成功时同时提交 Compacted
                input.compactions_left -= 1
                input.transcript = transcript::project(&entries)                             // 用新的 Transcript 再次组装
```

上一份回复计划的 blob 缺失或内容哈希不符时，`previous_plan = None`，在 `input.context.omitted` 中记录 `previous-plan:<hash>` 及不可读取的原因。composer 由此开始新的系列；其他存储错误与计划反序列化错误照常传播。

`exports = registry.exports()`，`target(用途)` 是 `profile.endpoint(用途).plugin` 通过 `completion(插件, safe_mode)` 取得的身份（11 §4.2）；找不到 → Failed(plugin.unavailable)。一个 Round 内目标插件由 profile 固定，代际可以变（§6.4），所以解析一次的请求在重试中仍然有效。

### 6.4 Attempt 与重试（`attempt.rs`）

模型请求与适配器以 Attempt 为单位固定（架构文档 §4.6）：每次 Attempt 开始时从导出表取当前代际，不从 Round 钉住的东西里取。

```text
attempt(endpoint, round, safe_mode, planned, token):
    plan_hash = store.put_blob(serde_json::to_vec(planned.plan))      // 先写 blob，再提交引用它的条目
    kind = planned.kind
    for n in 1..=MAX_ATTEMPTS:
        export = registry.exports().completion(endpoint.plugin, safe_mode)?   // 失败 → Ended(Failed(plugin.unavailable))，不写 AttemptStarted
        attempt_id = AttemptId::new()
        commit([AttemptStarted { round, attempt_id, purpose: kind 的目的, plan: plan_hash,
                                 composer: planned.composer,
                                 provider: export.generation, settings: endpoint.settings }])
        result = select {
            r = export.adapter.complete(&endpoint.settings, endpoint.api_key, planned.request.clone()) => r,
            _ = token.cancelled() => Err(Failure(cancelled))
        }
        match result:
            Ok(c) =>
                bodies = [AttemptSettled { Completed { c.message, c.usage, c.stop } }]
                empty_summary = false
                if kind == Compaction { upto }:
                    summary = c.message.joined_text()
                    empty_summary = summary.trim().is_empty()
                    if !empty_summary: bodies.push(Compacted { upto, summary, attempt: attempt_id })
                commit(bodies)
                registry.report(export.generation, Ok, Some(session))       // 11 §7；空摘要是压缩自己的判断，插件调用仍算成功
                if token.is_cancelled(): return Ended(Cancelled)
                if empty_summary: return Ended(Failed(compose.failed，说明 Provider 返回了空摘要))
                return Settled(c)
            Err(f) =>
                commit([AttemptSettled { Failed(f) }])
                rolled_back = registry.report(export.generation, Failed(f), Some(session)) // 回退与事件由注册表同事务提交
                if token.is_cancelled(): return Ended(Cancelled)
                if n == MAX_ATTEMPTS: return Ended(Failed(f))
                if rolled_back.is_some(): continue                            // 代际换了，立刻用新的再试，不退避
                if f.retryable:
                    select { sleep(BACKOFF[n-1]) => continue, token.cancelled() => return Ended(Cancelled) }
                return Ended(Failed(f))
```

- 重试复用同一份计划，不重新组装。目标插件由 profile 固定，所以解析过的请求对回退后的代际同样有效（§6.3）。
- 回退之后立即重试，是架构文档 §4.6 的"Provider 失败后改用健康代际，就是开始一次新的 Attempt"。回退事件与注册表状态同事务写入本 Session 的 Inbox，下一个 Round 消费，模型因此知道发生了什么（11 §7）。
- 导出表找不到插件时不写 `AttemptStarted`：没有代际可记。Round 以 `plugin.unavailable` 失败。
- 健康回报提交失败时 Session 停止（`KernelError::Registry`），与 Store 失败相同。
- 取消时直接丢弃 Provider 的 future：模型请求没有需要结算的外部效果。`AttemptSettled` 与健康回报照常提交之后，才以 `Cancelled` 结束。
- 内核不再另设超时，超时只在 Provider 实现中（07 §3.3）。

### 6.5 计划的校验与解析（`plan.rs`）

校验和解析分开：校验只在组装时做，组装与 `inspect`（§14）共用一个解析函数；重试复用已经解析的请求。

`pub(crate) fn validate(plan: &ContextPlan, input: &ComposeInput, purpose: AttemptPurpose, lifeline: &[CapabilityId]) -> Result<(), PlanError>`，按顺序检查：

1. `items` 非空。
2. 每个 `PlanItem::Log { pos }` 都必须是 `input.transcript.items` 中的某一项。
3. **工具配对**：按 `items` 解析出的消息序列中，每条包含工具调用的 Assistant 消息之后，紧跟的是恰好覆盖这些调用的 Tool 消息（顺序不限），然后才能出现其他消息；每条 Tool 消息都对应前面的某个调用。
4. `plan.tools` 中没有重复的名字，并且每一项都与 `input.tools` 中的某一项完全相同（`CapabilityId` 与 `ToolSpec` 都相同）。这保证记录下来的定义就是本轮 Snapshot 中的定义。
5. 目的为 `Reply` 且 `input.profile.requires_lifeline` 为真时，救生集中每个工具的 `CapabilityId` 都在 `plan.tools` 中。
6. 目的为 `Compaction` 时，`plan.tools` 必须为空。

`pub(crate) fn resolve(plan: &ContextPlan, transcript: &Transcript, exports: &Exports, target: PluginId) -> Result<ProviderRequest, PlanError>`：

- 每个 `PlanItem::Log { pos }` 解析为 `transcript.items` 中该项的消息与来源代际；`PlanItem::Message` 使用内联消息，来源代际为空。
- **扩展字段只回放给产生它的插件**：`Extension` 部分只在来源代际属于 `target` 身份时保留，否则去掉；没有来源的内联扩展同样去掉。内核不解释扩展字段，只决定给不给；插件收到的扩展字段一定是它自己的，不需要再过滤。
- 结果：`ProviderRequest { messages, tools: plan.tools 中的 ToolSpec（按顺序）, max_output_tokens }`。请求只来自计划、Log 与注册表，不从 Snapshot 读取工具定义。

**模型只能调用被披露的工具**：分派时只在 `plan.tools` 中查找（§6.6）。

### 6.6 工具分派（`dispatch.rs`）

```text
dispatch(snapshot, round, plan, call, token):
    if token.is_cancelled(): settle_without_start(call, Failed { "cancelled",
                                 "not executed: the run was cancelled before this call" }); return
    tool = plan.tools 中名为 call.name 的那一项；按它的 CapabilityId 取本轮 Snapshot 中的工具
    if 没有: settle_without_start(call, Failed { "tool.unavailable", "tool `{name}` is not available in this round" }); return
    args = match Arguments::parse(&call.arguments):
        Object(m) => m
        Invalid { raw } => settle_without_start(call, Failed { "tool.invalid_arguments",
                              "arguments must be a JSON object; received: {raw 的前 200 个字符}" }); return
    if tool.spec.check_argument_names(&args) 失败: settle_without_start(call, 该失败); return   // tool.invalid_arguments
    commit([ToolCallStarted { round, call: call.id, capability: tool.id, code: tool.code, effect: tool.spec.effect }])   // 预写
    outcome = tool.tool.call(CallContext { session, call: call.id, cancel: token.child_token(),
                                           result_budget: TOOL_RESULT_INLINE_BYTES }, args).await
    (content, full) = render(&outcome)
    commit([ToolCallSettled { call: call.id, outcome: Settlement::from(&outcome), content, full }])
```

- 工具按模型给出的顺序依次执行，目前不并行。
- `settle_without_start` 只提交 `ToolCallSettled`，不提交 `ToolCallStarted`：没有 Started 就意味着确定没有执行，恢复流程依赖这一点（§8）。
- 工具调用总是被等待到返回（不 drop），这就是"等待静止"。工具实现负责响应 `cancel`（05 §2）。
- 参数的归一化（`Arguments::parse`）与参数名检查（`ToolSpec::check_argument_names`，针对 `closed_object_schema` 构造的封闭对象）都只在这里。工具只校验自己读取的值，不再重复检查参数名，也不在开始前检查取消。

**结果文本**（`render`）：

| Outcome | 文本 |
|---|---|
| `Ok { value }` | `value` 是字符串时为该字符串，否则为格式化的 JSON |
| `Failed { failure }` | `error [{code}]: {message}` |
| `Unknown { failure }` | `outcome unknown [{code}]: {message}. The action may already have taken effect; check the current state before retrying.` |

**结果预算**：内核在 `CallContext.result_budget` 中给出结果可以内联的字节数（`TOOL_RESULT_INLINE_BYTES`）。能分页的工具在预算内按自己的单位停下，并说明如何继续（`fs_read` 见 05 §2.1）；内核不按工具身份区分，下面的截断对所有工具一样，是兜底。

文本超过 `TOOL_RESULT_INLINE_BYTES` 时：完整文本写入 blob，`full = Some(hash)`；`content` 为前 `TOOL_RESULT_PREVIEW_BYTES`（在字符边界截断）加上一行说明：`[truncated: {总字节数} bytes. Full result: {store.blob_path(hash)}; it may be cleaned up later. Read it with fs_read using offset and limit; if a single line is too long, read byte ranges with shell, for example head -c.]`。Log 只记录 `Settlement`、`content` 与 `full`，工具返回的原始值不另存（03 §1.6）。

生成文本的部分（不含 blob 写入）是一个纯函数 `render_text(&Outcome) -> String`，恢复流程复用它。

## 7. 安全模式

- 节点级开关，存于 Store 的 meta，由 `Kernel::set_safe_mode` 写入，在下一个 Round 开始时生效。
- 安全模式下，内核：不调用任何 ContextSource；只把救生集作为可披露的工具；在 `RoundStarted` 中记录 `safe_mode: true`；把 `ComposeInput.safe_mode` 设为真；Attempt 用出厂代际（`Exports::completion(插件, safe_mode = true)`，11 §4.2）。profile 不变，仍按它选插件与参数。
- composer 目前只有出厂实现，所以安全模式的效果是"最小上下文 + 只有救生集 + 出厂 Provider"。
- 自动进入安全模式目前不做：自动进入要等出现非出厂策略时才有意义。

## 8. 恢复（`recovery.rs`）

纯函数：`pub(crate) fn recover(entries: &[Entry]) -> Vec<EntryBody>`。结果由 actor 在启动时一次性提交。

1. 找到最后一条 `RunStarted`。如果之后有对应的 `RunEnded`，返回空。
2. 找到该 Run 中最后一条 `RoundStarted`。如果它没有对应的 `RoundEnded`：
   1. 该 Round 中每个没有 `AttemptSettled` 的 `AttemptStarted` → `AttemptSettled { Failed { "interrupted", retryable: true } }`。
   2. 取该 Round 中最后一个已完成的 Reply Attempt 的工具调用（如果有）。对每个调用：
      - 已有 `ToolCallSettled`：跳过。
      - 有 `ToolCallStarted`、没有 `ToolCallSettled`：`effect` 为 `ReadOnly` 时结算为 `Failed { "interrupted", retryable: true }`，否则为 `Unknown { "interrupted" }`。
      - 两者都没有：结算为 `Failed { "not_dispatched", "not executed: the process stopped before this call was dispatched" }`。
      - `content` 用 `render_text` 生成。
   3. `RoundEnded { Interrupted }`。
3. `RunEnded { Interrupted }`。

恢复**不会自动继续**被中断的 Run。被中断的状态通过工具结果与 composer 根据 `Transcript.run_ends` 生成的说明（05 §4.3）对模型可见；主人的下一条消息（或 Inbox 中尚未消费的 Event）会开启新的 Run。

这保证了：有副作用的工具不会被重新执行；已经被消费的 Event 不会被再次处理；尚未消费的 Event 在重启后照常处理。

## 9. 取消与关闭

- **是否开始由内核决定，如何停止由被调用方负责。** 内核只在两类位置检查令牌：决定是否开始下一步之前（每个上下文源之前、每次 Attempt 之前、每个工具调用之前，后者在 dispatch 中）；以及一个阶段结束后，由它决定 Round 的结局。上下文源与工具不在开始前自行检查。
- **一条规则**：内核把 Run 令牌的子令牌交给每个会等待外部的端口调用（ContextSource 经 `ContextQuery.cancel`，Tool 经 `CallContext.cancel`），然后等待调用返回；实现负责停止等待，让已经开始的本地写入完成，静止之后才返回。一个阶段结束时如果令牌已取消，本 Round 就以 `Cancelled` 结束，不看调用返回了什么。唯一直接丢弃 future 的是 Provider 请求，因为它不改变任何状态（§6.4）。
- `Kernel::cancel(session)`：如果 `run_cancel` 中有令牌就取消它。按上面的规则：上下文阶段中的来源停止等待后返回；正在进行的模型请求被丢弃并结算为 `cancelled`；正在执行的工具收到取消信号并自行结算；尚未执行的工具调用结算为 `cancelled`；最后写入 `RoundEnded { Cancelled }` 与 `RunEnded { Cancelled }`。
- `Kernel::shutdown()`：取消 Kernel 级的 shutdown 令牌（所有 Run 令牌的父令牌），等待每个 actor 的 task 与 Scheduler 的 task 结束，然后返回。被关闭打断的 Run 以 `Cancelled` 结束。
- `kill -9` 不经过这里，由 §8 在下次启动时处理。

## 10. Scheduler（`scheduler.rs`）

Scheduler 是一个 actor，是 schedules 表的唯一写者（架构文档 §3.6）。一次性提醒与周期定时是同一种东西：一条 Schedule 是一串触发时刻，`Once` 只有一个（03 §1.9）。

```rust
#[derive(Clone)]
pub struct Schedules {
    tx: mpsc::Sender<Command>,             // 私有命令枚举；容量 SCHEDULER_CHANNEL_CAPACITY
    stopped: Arc<Mutex<Option<String>>>,  // actor 报告，status 直接读取
}

impl Schedules {
    pub async fn create(&self, session: SessionId, rule: ScheduleRule, message: String) -> Result<Scheduled, ScheduleError>;
    pub async fn list(&self, session: Option<SessionId>) -> Result<Vec<Scheduled>, ScheduleError>;   // 只列 Active
    /// 停止之后的全部触发；已进入 Inbox 的照常处理。false 表示它已经不是 Active 或不存在。
    pub async fn cancel(&self, id: ScheduleId) -> Result<bool, ScheduleError>;
}

/// 给工具与 CLI 显示用：下一次触发是推出来的，不存储。
pub struct Scheduled {
    pub schedule: Schedule,
    pub next_due: DateTime<Utc>,
    pub timezone: Tz,                     // 本次查询时生效的时区，不存储
}

impl Scheduled {
    /// 定时工具与 CLI 共用的结果：`rule` 原样回显创建参数，时刻按生效时区写成带偏移的 RFC 3339。
    pub fn into_json(self) -> serde_json::Value;
}
```

### 10.1 下一次触发

```text
next_after(rule, after) -> Option<DateTime<Utc>>
    Once { at }               => at > after 时为 at，否则没有
    Cron { expr, timezone }   => 在 timezone（为空时用 KernelConfig.timezone）中，求 after 之后的第一个触发，转为 UTC

after = last.due_at；还没有触发过时为 created_at
```

- cron 的解析与求值用 croner，时区数据来自编译进二进制的 chrono-tz。使用 5 段规则（解析器的秒与年都设为 `Disallowed`），也接受解析器支持的别名，例如 `@daily`。
- 夏令时的空档与重叠按 croner 的规则处理：固定时刻的规则每天触发一次，间隔型规则按当地时间逐个匹配。
- 从 `last.due_at` 原样往后推，而不是从当前时刻，重复的那一小时才不会被算两遍。
- 主人的时区随配置改变后（重启生效），跟随型的定时从 `last` 起按新时区推算，指定了 `timezone` 的不受影响。所以向西改时区时，新时区当天已经过去的那次会立即补发（例如纽约 08:00 已投递后改为洛杉矶，会补发洛杉矶的 08:00），与宕机后的补发是同一条规则。

### 10.2 actor 循环

每个命令都带一个 oneshot 回复通道。actor 在内存中按 `(next_due, id)` 排列 Active 的 Schedule，解析后的规则也只由它持有，创建与启动时各解析一次；库中只有权威。

```text
启动：读出全部 Active 的 Schedule，按 10.1 算出各自的 next_due
tick = interval(SCHEDULER_TICK)，MissedTickBehavior::Delay     // 第一次 tick 立即执行，因此启动时补发错过的触发
loop select:
    cmd = rx.recv()          => create / list / cancel，同时更新内存
    _ = tick.tick()          => fire_due(clock.now())
    _ = shutdown.cancelled() => break

fire_due(now):
    for s in next_due <= now 的 Schedule，按 next_due 升序:
        due     = (after, now] 中最后一个触发时刻          // 宕机期间错过的触发只补最近一次
        skipped = (after, now] 中其余触发的个数
        next    = next_after(s.rule, due)
        event   = Event { id: EventId::new(), session: s.session, source: Scheduler,
                          body: Reminder { schedule: s.id, due_at: due, skipped, text: s.message },
                          received_at: now }
        store.fire_schedule(s.last.due_at, next 为空, &event)   // 一个事务：前移 last，必要时 Done，投递进 Inbox
        更新内存：last = (due, event.id)，next_due = next；next 为空时移出
        唤醒 s.session 的 actor
```

- "恰好触发一次"来自 `fire_schedule` 的单一事务，而不是来自时间判断：在提交之前崩溃，等于这次触发没有发生；提交之后，`last` 已经前移，重启后不会再投递同一次触发。
- `fire_schedule` 的前置条件不满足（`ScheduleNotActive`）说明内存与库不一致，只能是缺陷。它与其他 Store 错误一样按 §4.2 的规则停止 Scheduler；启动时无法还原的规则在错误中带上定时 ID。创建命令的参数错误只回复调用者。
- 时钟回拨时只是等待。休眠唤醒或时钟前跳之后，错过的触发落进"只补最近一次"。不设"太晚就不投递"的上限：模型从说明中看到触发时刻与到达时间，自己判断这次触发还有没有用。

### 10.3 创建的校验

- `message` 非空，Session 存在。
- 解析规则，从 `clock.now()` 起至少还有一次触发，否则返回 `NoOccurrenceAfter` 并带上该时刻。接下来 `RECURRENCE_CHECK` 次触发中，相邻两次的间隔不小于 `MIN_RECURRENCE_INTERVAL`；没有后续触发就结束检查。间隔下限拦住误写的规则，例如把每天八点写成 `* 8 * * *`，在八点那一小时每分钟唤醒一次 Agent。Once 与 Cron 走同一条路径。

```rust
#[derive(thiserror::Error)]
pub enum ScheduleError {
    #[error("reminder message is empty")]
    EmptyMessage,
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("the rule has no occurrence after {}", .0.to_rfc3339())]
    NoOccurrenceAfter(DateTime<FixedOffset>),
    #[error("occurrences must be at least {} minutes apart", MIN_RECURRENCE_INTERVAL.as_secs() / 60)]
    TooFrequent,
    #[error("schedule {id}: {source}")]
    Restore { id: ScheduleId, source: Box<ScheduleError> },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("scheduler has stopped{}", .0.as_ref().map_or(String::new(), |reason| format!(": {reason}")))]
    Stopped(Option<String>),
}
```

文本在进入的地方解析：工具把 `at` 解析为 `DateTime<Utc>`、把 `timezone` 解析为 `Tz`，失败时直接返回 `tool.invalid_arguments`。cron 表达式由 Scheduler 在创建时校验：永不触发与间隔过短都要对照当前时刻判断，而创建校验是归属者的事。

## 11. 内置工具（`builtin.rs`）

它们操作的是内核事实（Schedule），因此由内核提供，`CodeRef::Native { name: "kernel-builtin", version: crate 版本 }`。

| 名称 | 参数 | 结果 | Effect |
|---|---|---|---|
| `schedule_create` | `message`：提醒内容；`at`：带时区偏移的 RFC 3339 时间（一次性），或 `cron`：5 段 cron 表达式（周期），可加 `timezone`：IANA 名称，省略时跟随主人的时区；`at` 与 `cron` 恰好给一个 | `Ok`：创建的定时，与 `schedule_list` 中的一项相同；参数不合法、时间已经过去、规则永不触发或过于频繁 → `Failed { tool.invalid_arguments }`，message 说明原因 | SideEffect |
| `schedule_list` | 无 | `Ok { [ { id, session, rule, message, timezone, next_due, last_due } ] }`，仅当前 Session 的 Active 定时；`rule` 原样回显创建参数：`{ at }`、`{ cron }` 或 `{ cron, timezone }`；外层 `timezone` 是生效时区 | ReadOnly |
| `schedule_cancel` | `id` | `Ok { cancelled: bool }`，停止之后的全部触发；ID 无法解析 → `Failed { tool.invalid_arguments }` | SideEffect |

`at` 与 `cron` 二选一由工具检查，不写进 JSON Schema：并非所有服务商都支持 `oneOf`。

查询结果中的 `at`、`next_due`、`last_due` 按生效时区写成带偏移的 RFC 3339（05 §4.3）；`enco schedules` 共用这份查询视图。

`description` 以 `builtin.rs` 的源码为准，其中的间隔下限由常量生成。

## 12. 常量（`limits.rs`）

```rust
pub const MAX_ATTEMPTS: u32 = 3;
pub const BACKOFF: [Duration; MAX_ATTEMPTS as usize - 1] = [Duration::from_secs(1), Duration::from_secs(4)];
pub const MAX_COMPACTIONS_PER_ROUND: u32 = 2;
pub const TOOL_RESULT_INLINE_BYTES: usize = 16 * 1024;
pub const TOOL_RESULT_PREVIEW_BYTES: usize = 4 * 1024;
pub const SCHEDULER_TICK: Duration = Duration::from_secs(1);
pub const SESSION_BROADCAST_CAPACITY: usize = 256;
pub const SCHEDULER_CHANNEL_CAPACITY: usize = 64;
pub const MIN_RECURRENCE_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const RECURRENCE_CHECK: usize = 100;             // 创建时检查接下来这么多次触发的间隔（10.3）
pub const TRIAL_CALLS: u32 = 5;                      // 11 §7
```

## 13. 错误（`kernel.rs`）

```rust
#[derive(thiserror::Error)]
pub enum KernelError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error(transparent)]
    Registry(#[from] RegistryError),         // 11 §9
    #[error("configuration: {0}")]
    Config(String),                          // 只用于组合根输入不合法
    #[error("kernel is shutting down")]
    ShuttingDown,                            // 关闭开始后不再注册新的所有者
    #[error("owner task failed: {0}")]
    TaskFailed(String),                      // 所有者任务没有经过正常关闭路径就退出
    #[error("session {0} is stopped: {1}")]
    SessionStopped(SessionId, String),
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    #[error("unknown profile {0}")]
    UnknownProfile(String),
    #[error("session {0} has no attempt {1}")]
    UnknownAttempt(SessionId, String),       // inspect；第二项是 attempt id 或 "latest"
    #[error("attempt {attempt}: invalid recorded plan: {reason}")]
    InvalidPlan { attempt: AttemptId, reason: String },
}
```

`PlanError`、`ComposeError`、`ContextError` 不会穿出内核：它们在 Round 内被转换为 `Failure` 并记录为 `RoundEnded { Failed }`；查看已记录计划的解析错误由 `KernelError::InvalidPlan` 报告。

## 14. 请求查看：`Kernel::inspect`

把一次 Attempt 记录的东西解析成模型实际收到的请求。它和 Round 用同一个 `plan::resolve`，所以看到的就是发出去的；A2 的验收（09 §4）用它核对。

```rust
pub struct Inspection {
    pub attempt: AttemptId,
    pub purpose: AttemptPurpose,
    pub composer: CodeRef,
    pub provider: GenerationId,
    pub settings: ProviderSettings,
    pub request: ProviderRequest,     // 解析后的消息、工具定义与输出上限
    pub plan: ContextPlan,            // 原始计划：内联消息的候选来源、Log 引用与省略
    pub result: Option<AttemptResult>,
}
```

```text
inspect(session, attempt):
    entries = store.log(session, None)
    started = attempt 指定时找那条 AttemptStarted，否则取最后一条；没有 → UnknownAttempt
    plan = store.get_blob(started.plan)
    transcript = transcript::project(位置小于 started.pos 的条目)    // 组装时看到的历史
    target = registry.exports().plugin_of(started.provider)            // 当时用的插件；注册表没有这个代际 → StoreError::UnknownGeneration
    request = plan::resolve(&plan, &transcript, &exports, target)
    result = 对应的 AttemptSettled（如果有）
```

`Inspection.plan` 原样返回已记录的计划，来源跟着它所属的内联消息一起呈现，只记在计划里，不发给 Provider。

Transcript 只取 Attempt 之前的条目，所以同一 Round 里压缩前后的两次 Attempt 各自解析出当时的历史。计划引用的 blob 已被清理时返回 `StoreError::Blob`，不猜。
