# 04 内核（enco-kernel）

内核只负责四件事：**记录**（Log）、**绑定**（本轮用哪份代码）、**校验**（计划与工具调用）、**调度**（Session actor、Run/Round、Scheduler）。它不包含 IO 实现、上下文提示词或排版，也不按具体工具、Provider 或服务身份写分派逻辑（01 §2）。内核自己的 Schedule 命令经普通 Tool 端口接入（§11）。

内核也不知道记忆：记忆是宿主经 Tool 与 ContextSource 两个端口接入的能力（06）。

## 1. 架构形态

端口与适配器（hexagonal architecture）：内核通过六个端口与外界交互，适配器由宿主与 Wasm 层实现，在组合根一次性装配。

```text
            ┌──────────────── enco-kernel ────────────────┐
 CLI/守护进程 │  Kernel ── Session actor ── Run/Round ──┐    │
 ──submit──▶ │     │          (每 Session 一个)         │    │
             │  Scheduler actor                         │    │
             │     ports: Store · Provider · Composer · ContextSource · Tool · Clock
             └─────────┬────────┬─────────┬──────────┬─────┘
                  enco-host  enco-wasm  enco-host  enco-host / 内核内置
                  (SQLite)  (Provider)  (composer、上下文、fs/shell、记忆)
```

## 2. 端口

六个端口都位于真实的边界上（IO、插件、策略、时间）。Store 的完整定义见 03 §2。

```rust
// ports/provider.rs
#[async_trait]
pub trait Provider: Send + Sync {
    /// 当前提供服务的代码，写入 AttemptStarted。
    fn code(&self) -> CodeRef;
    /// 一次非流式补全。超时由实现负责，以 Failure { code: "timeout", retryable: true } 返回。
    async fn complete(&self, request: ProviderRequest) -> Result<Completion, Failure>;
    /// 把若干段文本转换为向量，结果与输入一一对应。内核自己不调用它，它供宿主使用（06 §3.3）。
    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, Failure>;
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
// ports/composer.rs
pub trait Composer: Send + Sync {
    fn code(&self) -> CodeRef;
    /// 纯函数：同样的输入得到同样的输出。不做 IO，不读时钟，不用随机数，因此是同步的。
    fn compose(&self, input: &ComposeInput) -> Result<Composition, ComposeError>;
}

pub struct ComposeInput {
    pub now: DateTime<FixedOffset>,        // 本 Round 读取的时刻与宿主当时的 UTC 偏移（Clock）
    pub session: SessionRecord,
    pub transcript: Transcript,
    pub previous_run_end: Option<RunEnd>,  // 上一个 Run 的结束方式，用于提示中断等情况
    pub context: Contribution,             // 各 ContextSource 的贡献按顺序合并；安全模式下为空
    pub tools: Vec<(CapabilityId, ToolSpec)>, // 本轮可披露的工具；安全模式下只有救生集
    pub safe_mode: bool,
    pub budget: Budget,
}

pub struct Budget { pub context_tokens: u32, pub max_output_tokens: u32 }

/// Log 的模型视图，由内核投影（§5）。
pub struct Transcript {
    pub summary: Option<String>,        // 最近一次 Compacted 的摘要
    pub items: Vec<TranscriptItem>,     // 该次压缩之后、具有规范消息形态的条目，按 Log 位置递增
    pub round_ends: Vec<LogPos>,        // 该次压缩之后的 RoundEnded 位置，按 Log 位置递增，也就是合法的压缩边界
}

pub struct TranscriptItem { pub pos: LogPos, pub message: Message }

pub enum Composition {
    Plan(ContextPlan),
    /// 请求先压缩：`upto` 必须属于 `round_ends`，`plan` 是生成摘要的请求（不带工具）。
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
    /// 没有新 Event 被消费时，召回的查询文本不变；来源读取的当前状态仍可能变化。
    pub latest_event: Option<Event>,
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
    /// 当前时刻，带宿主此刻的 UTC 偏移。时刻与偏移在同一次调用中取得，内核不读取系统时区。
    /// 调用方存储的时间（Log、Inbox、Schedule、记忆）一律转为 UTC（`to_utc()`）。
    fn now(&self) -> DateTime<FixedOffset>;
}
```


## 3. Kernel（`kernel.rs`）

```rust
pub struct KernelDeps {
    pub store: Arc<dyn Store>,
    pub provider: Arc<dyn Provider>,
    pub composer: Arc<dyn Composer>,
    pub context: Vec<Arc<dyn ContextSource>>,
    pub tools: Vec<Arc<dyn Tool>>,   // 宿主提供的工具；内核再加入自己的内置工具（§11）
    pub lifeline: Vec<String>,       // 组成救生集的工具名
    pub clock: Arc<dyn Clock>,
}

/// 字段不公开，只能经 `KernelConfig::new` 构造：预算的合法性只在这里定义一次。
pub struct KernelConfig {
    budget: Budget,
    max_rounds_per_run: u32,
}

impl KernelConfig {
    /// Round 数与预算必须为正，且输出上限小于窗口，否则返回 `KernelError::Config`。
    /// 组合根读完配置立即构造它，所以不合法的配置在打开存储与记忆之前就失败。
    pub fn new(budget: Budget, max_rounds_per_run: u32) -> Result<Self, KernelError>;
}

impl Kernel {
    /// 构造 Snapshot，启动 Scheduler，为每个已有 Session 启动 actor（actor 启动时自行恢复）。
    pub async fn start(deps: KernelDeps, config: KernelConfig) -> Result<Kernel, KernelError>;
    /// 不存在则创建，并确保它的 actor 已经启动。
    pub async fn open_session(&self, name: &str) -> Result<SessionRecord, KernelError>;
    /// 构造 `Event { source: Cli, body: UserMessage, received_at: clock.now().to_utc() }`，持久接纳并唤醒 Session。
    /// 重复的 `event_id` 返回 Duplicate。
    pub async fn submit(&self, session: SessionId, event_id: EventId, text: String) -> Result<Accepted, KernelError>;
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
    pub provider: CodeRef,
    pub composer: CodeRef,
    pub sessions: Vec<SessionStatus>,
}

pub struct SessionStatus { pub session: SessionRecord, pub running: bool, pub stopped: Option<String> }
```

守护进程与内置工具调用的是同一组方法：主人执行 `enco schedules --cancel` 与 Agent 调用 `schedule_cancel`，走的是同一个 `Schedules::cancel`。

## 4. Snapshot 与 Session actor

### 4.1 Snapshot（`snapshot.rs`）

`Kernel::start` 构造一次，之后不变（P1 才引入替换）。

```rust
pub(crate) struct Snapshot {
    pub provider: Arc<dyn Provider>,
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
    pub wake: Arc<Notify>,                          // notify_one 会保留许可，不会丢失唤醒
    pub entries: broadcast::Sender<Entry>,
    pub run_cancel: Mutex<Option<CancellationToken>>, // 正在进行的 Run 的取消令牌
    pub stopped: Mutex<Option<String>>,             // actor 因错误退出时的原因
    pub task: JoinHandle<()>,
}
```

Kernel 持有 `Mutex<HashMap<SessionId, SessionHandle>>`。actor 的状态包括 `SessionRecord`、该 Session 全部条目的内存副本（启动时加载，提交后追加）、下一个位置 `next: LogPos`。

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

**提交辅助函数** `commit(bodies, consumed)`：从 `next` 开始依次分配位置，`at = clock.now().to_utc()`，调用 `store.commit`；成功后追加到内存副本，并把每个条目发送到 broadcast（没有订阅者时忽略发送错误）。

**错误处理**：`store.commit` 返回的任何错误都说明前提已被破坏（被 fence、位置错乱或存储故障）。actor 记录错误、写入 `stopped`，然后退出。不要尝试继续或修补；重启后恢复流程会处理。

## 5. Transcript 投影（`transcript.rs`）

纯函数：`pub(crate) fn project(entries: &[Entry]) -> Transcript`。

1. 找到最近一条 `Compacted`。位置不大于它的 `upto` 的条目被隐藏，`summary` 取它的摘要；没有则 `summary = None`、不隐藏任何条目。
2. 遍历**全部**条目，记录每个 Attempt 的目的（来自 `AttemptStarted`）和每个 CallId 的 `provider_id`（来自已完成的 Reply 消息中的 `ToolCall`）。
3. 对未被隐藏的条目，按 Log 顺序：
   - 具有规范消息形态的条目（03 §1.5 的表）生成 `TranscriptItem`；目的为 `Compaction` 的 Attempt 不生成。
   - `RoundEnded` 的位置加入 `round_ends`。

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
    snapshot = self.snapshot.clone()               // 本轮钉住
    safe_mode = store.node().safe_mode             // 安全模式在 Round 边界生效
    pending = store.pending(session)
    commit(prefix ++ [EventConsumed { e } for e in pending] ++ [RoundStarted { run, round_id, safe_mode }],
           consumed = pending 的 id)

    (plan, request) = compose(snapshot, round_id, safe_mode, token)?      // §6.3；取消或失败则 end_round
    completion = attempt(snapshot, round_id, Reply, plan, request, token)? // §6.4；取消或失败则 end_round
    calls = completion.message.tool_calls()
    if calls.is_empty(): return end_round(Replied)
    for call in calls: dispatch(snapshot, round_id, plan, call, token)   // §6.6：是否开始由 dispatch 决定
    return end_round(if token.is_cancelled() { Cancelled } else { ToolsSettled })
```

`end_round(end)` 提交 `RoundEnded { round, end }` 并返回 `end`。

### 6.3 组装与压缩

组装分两个时刻。**组装时**读取当前状态：时间与上下文贡献每个 Round 读取一次，压缩只改变 Transcript，重新组装时复用它们。**记录后**只解析记录：Attempt 与重试使用冻结的计划（§6.4），不再调用 ContextSource 或读取 Snapshot 中的工具定义。

```text
compose(snapshot, round, safe_mode, token):
    now = clock.now()                                          // 每个 Round 读取一次，偏移随宿主当前时区
    context = if safe_mode { Contribution::default() }
              else { 以 ContextQuery { session, latest_event, cancel: token.child_token() } 依次调用每个 ContextSource，
                     按顺序合并 candidates 与 omitted }
    if token.is_cancelled(): return Ended(Cancelled)          // 无论来源返回了什么（§9）
    任何一个来源出错 → Failed(context.failed)
    for _ in 0..=MAX_COMPACTIONS_PER_ROUND:
        input = ComposeInput {
            now, session, transcript: transcript::project(&entries),
            previous_run_end: 当前 Run 之前最后一条 RunEnded 的 end,
            context: context.clone(),
            tools: if safe_mode { 救生集 } else { 全部 },
            safe_mode, budget,
        }
        match composer.compose(&input):
            Err(e)                    => Failed(e 转为 Failure：ContextOverflow → context.overflow，其余 → compose.failed)
            Ok(Plan(plan))            => return (plan, plan::resolve(&plan, &input, Reply, &snapshot.lifeline)?)   // 校验失败 → Failed(plan.invalid)
            Ok(Compact { upto, plan }) =>
                upto 必须属于 input.transcript.round_ends，否则 Failed(plan.invalid)
                request = plan::resolve(&plan, &input, Compaction, &snapshot.lifeline)?
                attempt(snapshot, round, Compaction { upto }, plan, request, token)?   // 成功时同时提交 Compacted
                // 继续循环：用新的 Transcript 再次组装
    Failed(compose.failed, "too many compactions in one round")
```

### 6.4 Attempt 与重试（`attempt.rs`）

```text
attempt(snapshot, round, kind, plan, request, token):
    plan_hash = store.put_blob(serde_json::to_vec(plan))      // 先写 blob，再提交引用它的条目
    for n in 1..=MAX_ATTEMPTS:
        attempt_id = AttemptId::new()
        commit([AttemptStarted { round, attempt_id, purpose: kind 的目的, plan: plan_hash,
                                 composer: snapshot.composer.code(), provider: snapshot.provider.code() }])
        result = select {
            r = snapshot.provider.complete(request.clone()) => r,
            _ = token.cancelled() => { commit([AttemptSettled { Failed(cancelled) }]); return Ended(Cancelled) }
        }
        match result:
            Ok(c) =>
                bodies = [AttemptSettled { Completed { c.message, c.usage, c.stop } }]
                if kind == Compaction { upto }:
                    summary = c.message.joined_text()
                    if summary.trim().is_empty(): commit(bodies); return Ended(Failed(compose.failed, "empty summary"))
                    bodies.push(Compacted { upto, summary, attempt: attempt_id })
                commit(bodies); return Settled(c)
            Err(f) =>
                commit([AttemptSettled { Failed(f) }])
                if f.retryable && n < MAX_ATTEMPTS:
                    select { sleep(BACKOFF[n-1]) => continue, token.cancelled() => return Ended(Cancelled) }
                return Ended(Failed(f))
```

- 重试复用同一份计划，不重新组装。
- 取消时直接丢弃 Provider 的 future：模型请求没有需要结算的外部效果。
- 内核不再另设超时，超时只在 Provider 实现中（07 §3.3）。

### 6.5 计划的校验与解析（`plan.rs`）

`pub(crate) fn resolve(plan: &ContextPlan, input: &ComposeInput, purpose: AttemptPurpose, lifeline: &[CapabilityId]) -> Result<ProviderRequest, PlanError>`，按顺序检查：

1. `items` 非空。
2. 每个 `PlanItem::Log { pos }` 都必须是 `input.transcript.items` 中的某一项，解析为它的消息；`PlanItem::Message` 原样使用。
3. **工具配对**：解析后的消息序列中，每条包含工具调用的 Assistant 消息之后，紧跟的是恰好覆盖这些调用的 Tool 消息（顺序不限），然后才能出现其他消息；每条 Tool 消息都对应前面的某个调用。
4. `plan.tools` 中没有重复的名字，并且每一项都与 `input.tools` 中的某一项完全相同（`CapabilityId` 与 `ToolSpec` 都相同）。这保证记录下来的定义就是本轮 Snapshot 中的定义。
5. 目的为 `Reply` 且 `session.config.requires_lifeline` 为真时，救生集中每个工具的 `CapabilityId` 都在 `plan.tools` 中。
6. 目的为 `Compaction` 时，`plan.tools` 必须为空。

结果：`ProviderRequest { messages, tools: plan.tools 中的 ToolSpec（按顺序）, max_output_tokens }`。请求只来自计划与 Log，不从 Snapshot 读取工具定义。

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

- 工具按模型给出的顺序依次执行，P0 不并行。
- `settle_without_start` 只提交 `ToolCallSettled`，不提交 `ToolCallStarted`：没有 Started 就意味着确定没有执行，恢复流程依赖这一点（§8）。
- 工具调用总是被等待到返回（不 drop），这就是"等待静止"。工具实现负责响应 `cancel`（05 §2）。
- 参数的归一化（`Arguments::parse`）与参数名检查（`ToolSpec::check_argument_names`，针对 `closed_object_schema` 构造的封闭对象）都只在这里。工具只校验自己读取的值，不再重复检查参数名，也不在开始前检查取消。

**结果文本**（`render`）：

| Outcome | 文本 |
|---|---|
| `Ok { value }` | `value` 是字符串时为该字符串，否则为格式化的 JSON |
| `Failed { failure }` | `error [{code}]: {message}` |
| `Unknown { failure }` | `outcome unknown [{code}]: {message}. The action may already have taken effect; check the current state before retrying.` |

**结果预算**：内核在 `CallContext.result_budget` 中给出结果可以内联的字节数（P0 为 `TOOL_RESULT_INLINE_BYTES`）。能分页的工具在预算内按自己的单位停下，并说明如何继续（`fs_read` 见 05 §2.1）；内核不按工具身份区分，下面的截断对所有工具一样，是兜底。

文本超过 `TOOL_RESULT_INLINE_BYTES` 时：完整文本写入 blob，`full = Some(hash)`；`content` 为前 `TOOL_RESULT_PREVIEW_BYTES`（在字符边界截断）加上一行说明：`[truncated: {总字节数} bytes. Full result: {store.blob_path(hash)}; it may be cleaned up later. Read it with fs_read using offset and limit; if a single line is too long, read byte ranges with shell, for example head -c.]`。Log 只记录 `Settlement`、`content` 与 `full`，工具返回的原始值不另存（03 §1.6）。

生成文本的部分（不含 blob 写入）是一个纯函数 `render_text(&Outcome) -> String`，恢复流程复用它。

## 7. 安全模式

- 节点级开关，存于 Store 的 meta，由 `Kernel::set_safe_mode` 写入，在下一个 Round 开始时生效。
- 安全模式下，内核：不调用任何 ContextSource；只把救生集作为可披露的工具；在 `RoundStarted` 中记录 `safe_mode: true`；把 `ComposeInput.safe_mode` 设为真。
- P0 的 composer 与 Provider 本来就是出厂代码，所以安全模式在 P0 中的效果是"最小上下文 + 只有救生集"。
- 自动进入安全模式不在 P0 范围内：自动进入要等出现非出厂策略时才有意义。

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

恢复**不会自动继续**被中断的 Run。被中断的状态通过工具结果和 `previous_run_end` 对模型可见；主人的下一条消息（或 Inbox 中尚未消费的 Event）会开启新的 Run。

这保证了：有副作用的工具不会被重新执行；已经被消费的 Event 不会被再次处理；尚未消费的 Event 在重启后照常处理。

## 9. 取消与关闭

- **是否开始由内核决定，如何停止由被调用方负责。** 内核只在两类位置检查令牌：决定是否开始下一步之前（每个上下文源之前、每次 Attempt 之前、每个工具调用之前，后者在 dispatch 中）；以及一个阶段结束后，由它决定 Round 的结局。上下文源与工具不在开始前自行检查。
- **一条规则**：内核把 Run 令牌的子令牌交给每个会等待外部的端口调用（ContextSource 经 `ContextQuery.cancel`，Tool 经 `CallContext.cancel`），然后等待调用返回；实现负责停止等待，让已经开始的本地写入完成，静止之后才返回。一个阶段结束时如果令牌已取消，本 Round 就以 `Cancelled` 结束，不看调用返回了什么。唯一直接丢弃 future 的是 Provider 请求，因为它不改变任何状态（§6.4）。
- `Kernel::cancel(session)`：如果 `run_cancel` 中有令牌就取消它。按上面的规则：上下文阶段中的来源停止等待后返回；正在进行的模型请求被丢弃并结算为 `cancelled`；正在执行的工具收到取消信号并自行结算；尚未执行的工具调用结算为 `cancelled`；最后写入 `RoundEnded { Cancelled }` 与 `RunEnded { Cancelled }`。
- `Kernel::shutdown()`：取消 Kernel 级的 shutdown 令牌（所有 Run 令牌的父令牌），等待每个 actor 的 task 与 Scheduler 的 task 结束，然后返回。被关闭打断的 Run 以 `Cancelled` 结束。
- `kill -9` 不经过这里，由 §8 在下次启动时处理。

## 10. Scheduler（`scheduler.rs`）

Scheduler 是一个 actor，是 schedules 表的唯一写者。

```rust
#[derive(Clone)]
pub struct Schedules { tx: mpsc::Sender<SchedulerCmd> }   // 容量 SCHEDULER_CHANNEL_CAPACITY

impl Schedules {
    pub async fn create(&self, session: SessionId, due_at: DateTime<Utc>, message: String) -> Result<Schedule, ScheduleError>;
    pub async fn list(&self, session: Option<SessionId>) -> Result<Vec<Schedule>, ScheduleError>;   // 只列 Pending
    pub async fn cancel(&self, id: ScheduleId) -> Result<bool, ScheduleError>;
}
```

每个命令都带一个 oneshot 回复通道。actor 循环：

```text
tick = interval(SCHEDULER_TICK)，MissedTickBehavior::Delay     // 第一次 tick 立即触发，因此启动时会补发过期的提醒
loop select:
    cmd = rx.recv()        => 处理 create / list / cancel
    _ = tick.tick()        => fire_due(clock.now().to_utc())
    _ = shutdown.cancelled() => break

fire_due(now):
    for s in store.schedules(Pending)，按 due_at 升序，且 s.due_at <= now:
        event = Event { id: EventId::new(), session: s.session, source: Scheduler,
                        body: Reminder { schedule: s.id, due_at: s.due_at, text: s.message }, received_at: now }
        store.fire_schedule(s.id, &event)      // 一个事务：Pending → Fired，并投递进 Inbox
        唤醒 s.session 的 actor
```

- `create` 的校验：`due_at` 必须晚于 `clock.now()`，`message` 非空，Session 存在；否则返回 `ScheduleError`。

```rust
#[derive(thiserror::Error)]
pub enum ScheduleError {
    #[error("due time {0} is not in the future")]
    InPast(DateTime<Utc>),
    #[error("reminder message is empty")]
    EmptyMessage,
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("scheduler has stopped")]
    Stopped,
}
```
- "恰好触发一次"来自 `fire_schedule` 的单一事务，而不是来自时间判断。

## 11. 内置工具（`builtin.rs`）

它们操作的是内核事实（Schedule），因此由内核提供，`CodeRef::Native { name: "kernel-builtin", version: crate 版本 }`。

| 名称 | 参数 | 结果 | Effect |
|---|---|---|---|
| `schedule_create` | `at`：带时区偏移的 RFC 3339 时间；`message`：提醒内容 | `Ok { id, due_at }`；时间无法解析或已经过去 → `Failed { tool.invalid_arguments }` | SideEffect |
| `schedule_list` | 无 | `Ok { [ { id, due_at, message } ] }`，仅当前 Session 的 Pending 提醒 | ReadOnly |
| `schedule_cancel` | `id` | `Ok { cancelled: bool }`；ID 无法解析 → `Failed { tool.invalid_arguments }` | SideEffect |

`description` 是模型看到的文字，写清用途与参数格式即可，例如 `schedule_create`：`Create a reminder. It will be delivered to this session at the given time, even if Enco restarts. "at" must be an RFC 3339 timestamp with a UTC offset.`

## 12. 常量（`limits.rs`）

```rust
pub const MAX_ATTEMPTS: u32 = 3;
pub const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];
pub const MAX_COMPACTIONS_PER_ROUND: u32 = 2;
pub const TOOL_RESULT_INLINE_BYTES: usize = 16 * 1024;
pub const TOOL_RESULT_PREVIEW_BYTES: usize = 4 * 1024;
pub const SCHEDULER_TICK: Duration = Duration::from_secs(1);
pub const SESSION_BROADCAST_CAPACITY: usize = 256;
pub const SCHEDULER_CHANNEL_CAPACITY: usize = 64;
```

## 13. 错误（`kernel.rs`）

```rust
#[derive(thiserror::Error)]
pub enum KernelError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
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
}
```

`PlanError`、`ComposeError`、`ContextError` 不会穿出内核：它们在 Round 内被转换为 `Failure` 并记录为 `RoundEnded { Failed }`。
