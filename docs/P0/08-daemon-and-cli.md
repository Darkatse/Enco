# 08 守护进程与 CLI（apps/enco）

`apps/enco` 是组合根：读取配置，构造全部适配器，启动 Kernel，并通过本地协议对外提供服务。CLI 的聊天与管理命令都是这个协议的客户端。它们与 Agent 的内置工具调用的是同一组 Kernel 方法（04 §3）。

## 1. 子命令

| 命令 | 作用 |
|---|---|
| `enco init` | 创建 `ENCO_HOME`、工作区目录与配置模板（已存在的文件不覆盖），打印下一步 |
| `enco serve` | 在前台运行守护进程 |
| `enco chat [--session <name>]` | 交互式对话，默认 Session 为 `main` |
| `enco send [--session <name>] <text>` | 发送一条消息后退出，不等待回复 |
| `enco status` | 节点、安全模式、代码版本、各 Session 状态 |
| `enco sessions` | 列出 Session |
| `enco log [--session <name>]` | 输出 Session 的全部条目 |
| `enco safe-mode <on\|off>` | 切换安全模式（下一个 Round 生效） |
| `enco schedules [--cancel <id>]` | 列出待触发的提醒，或取消一个 |
| `enco memory [--forget <id>]` | 列出全部记忆（标出置顶与尚未进入索引的），或遗忘一条 |
| `enco cancel [--session <name>]` | 取消正在进行的 Run |

除 `init` 与 `serve` 外，所有命令都连接正在运行的守护进程；连接失败时提示 `enco serve is not running`。管理命令不经过模型，模型不可用时照样可以使用。

管理命令把协议返回的 `data` 输出为格式化的 JSON，这是唯一的输出形式；验收测试直接解析它（09 §4）。

## 2. 路径（`paths.rs`）

`ENCO_HOME` 环境变量，默认为 `dirs::home_dir()/.enco`。

```text
$ENCO_HOME/
  config.toml
  enco.db
  memory.db          记忆的权威（06 §2）
  memory-index/      记忆索引，派生物，随时可以删除（06 §3）
  enco.sock
  blobs/
  workspace/
```

## 3. 配置（`config.rs`）

`$ENCO_HOME/config.toml`，结构体全部 `#[serde(deny_unknown_fields)]`：

```toml
[provider]                        # 必填
plugin = "openai-compatible"      # 可选；openai-compatible 或 deepseek
base_url = "https://api.openai.com/v1"
model = "<model name>"
api_key_env = "OPENAI_API_KEY"    # 可选；不需要认证的本地服务可以省略
options = {}                      # 可选；合并进请求体，例如 { temperature = 0.7 }

[embedding]                       # 必填；记忆使用（06）
plugin = "openai-compatible"      # 可选，默认 openai-compatible
base_url = "https://api.openai.com/v1"
model = "text-embedding-3-small"
dimensions = 1536                 # 模型返回的向量维度；与现有索引不一致时，索引自动重建
api_key_env = "OPENAI_API_KEY"    # 可选
options = {}                      # 可选；合并进请求体，例如支持缩短维度的模型可写 { dimensions = 512 }

[context]                         # 可选
window_tokens = 128000            # 默认 128000
max_output_tokens = 8192          # 默认 8192

[run]                             # 可选
max_rounds = 24                   # 默认 24
```

- 配置文件不存在：报错并提示运行 `enco init`。
- 缺少 `[provider]` 或 `[embedding]`：报错并拒绝启动。`[embedding]` 可以指向与 `[provider]` 不同的服务，也可以指向本地的 OpenAI 兼容服务（例如 Ollama）。
- 给出了 `api_key_env` 但该环境变量未设置：报错并拒绝启动。API key 只从环境变量读取，不写进配置文件。
- 这是 P0 的全部配置项。新增配置项需要先提问（01 §5）。

## 4. 组合根（`compose_root.rs`）

```text
paths  = Paths::from_env()
(config, embedding) = Config::load(paths.config())                          // 一次得到校验过的 EmbeddingSpec
kernel_config = KernelConfig::new(budget, config.run.max_rounds, 本机时区偏移)?  // 预算规则只在内核定义（04 §3），在打开任何资源之前失败
确保 workspace/ 存在
clock  = Arc::new(SystemClock)
store  = SqliteStore::open(paths.db(), paths.blobs())
provider_bytes  = factory(config.provider.plugin); embedding_bytes = factory(config.embedding.plugin)   // 07 §5
store.put_blob(provider_bytes); store.put_blob(embedding_bytes)
engine   = Arc::new(WasmEngine::new())
provider = WasmProvider::new(engine.clone(), provider_bytes, [provider] 的设置)
embedder = WasmProvider::new(engine, embedding_bytes, [embedding] 的设置)
memories = Memories::open(MemoryPaths { db: paths.memory_db(), index: paths.memory_index() },
                          embedding, embedder, clock.clone())               // 06 §7
deps = KernelDeps {
    store, provider,
    composer: FactoryComposer::new(paths.workspace()),
    context:  vec![WorkspaceContextSource::new(paths.workspace()), MemoryContextSource::new(memories.clone())],
    tools:    enco_host::native_tools(paths.workspace()) ++ enco_host::memory_tools(memories.clone()),
    lifeline: enco_host::LIFELINE 转为 Vec<String>,
    clock,
}
kernel = Kernel::start(deps, kernel_config)
```

依赖在这里一次性构造成具体的结构体。不要引入注册表、工厂或容器。

## 5. 守护进程（`daemon.rs`）

启动：

1. 解析路径。如果 `enco.sock` 已存在：尝试连接；连得上说明已有守护进程在运行，报错退出；连不上说明是残留文件，删除它。
2. 绑定 Unix domain socket，并把文件权限设为 `0600`（这个 socket 等同于对 Agent 的完全控制权）。绑定失败就退出，尚不打开存储或启动任务。
3. 执行组合根。必须先持有 socket，再打开数据库与记忆索引、启动 Kernel；避免第二次 `enco serve` 先触发索引重建或 Session 恢复，再发现已有进程。组合根失败时关闭并删除本进程的 socket，返回错误。
4. 接受循环：每个连接一个任务。
5. 收到 SIGINT 或 SIGTERM：停止接受新连接，调用 `kernel.shutdown()` 并等待它完成，删除 socket 文件，退出。

每个连接的任务：按行读取请求，依次处理，写回响应。一个连接最多订阅一个 Session；订阅之后，同一个任务用 `select!` 同时处理新的请求行和 broadcast 中的条目。所有写出都经过这一个任务，因此一个连接上的输出不会交错。

broadcast 报告 `Lagged(n)` 时，推送 `{"type":"error","id":0,"code":"lagged","message":"missed {n} entries; run `enco log` to resync"}`，然后继续。

## 6. 本地协议（`protocol.rs`）

每行一个 JSON 对象，UTF-8，以 `\n` 结束。

### 6.1 请求

```rust
#[derive(Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// 由客户端生成 event_id，重发同一条消息是幂等的。
    Send { session: String, text: String, event_id: EventId },
    Subscribe { session: String },
    Cancel { session: String },
    Status {},
    Sessions {},
    Log { session: String, after: Option<LogPos> },
    SafeMode { enabled: bool },
    Schedules {},
    CancelSchedule { schedule_id: ScheduleId },
    Memories {},
    ForgetMemory { memory_id: MemoryId },
}
```

**命名规则**：裸 `id` 只属于请求信封，用来关联请求与响应；命令中引用某个对象的字段一律写作 `<种类>_id`，与已有的 `event_id` 一致。例如 `{"id":1,"cmd":"cancel_schedule","schedule_id":"<ULID>"}`。工具参数没有信封，仍然使用 `id`。

`deny_unknown_fields` 放在 `Command` 上；无参数命令使用空字段变体，使其同样拒绝多余字段。不要在带 `flatten` 的外层 `Request` 上再加该属性，否则正常的 `cmd` 也会被拒绝。JSON 形状仍为 `{"id":1,"cmd":"status"}`。

`send` 与 `subscribe` 在 Session 不存在时创建它（`Kernel::open_session`）；其余命令遇到不存在的 Session 返回 `unknown_session`。

`memories` 与 `forget_memory` 直接调用 `Memories::list` 与 `Memories::forget`，与 Agent 的记忆工具是同一组方法（06 §1）；其余命令调用 Kernel。

### 6.2 服务端消息

```rust
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ok { id: u64, data: serde_json::Value },
    Error { id: u64, code: String, message: String },
    Entry { session: String, entry: Entry },   // 订阅之后推送
}
```

| 命令 | `data` |
|---|---|
| `send` | `{ "session_id", "event_id", "accepted": "new" \| "duplicate" }` |
| `subscribe` | `{ "session_id" }` |
| `cancel` | `{ "cancelled": bool }` |
| `status` | `Status` |
| `sessions` | `[SessionRecord]` |
| `log` | `[Entry]` |
| `safe_mode` | `{ "enabled": bool }` |
| `schedules` | `[Schedule]`（仅 Pending） |
| `cancel_schedule` | `{ "cancelled": bool }` |
| `memories` | `MemoryList`（06 §7） |
| `forget_memory` | `{ "forgotten": bool }` |

错误码：`bad_request`（无法解析的请求）、`unknown_session`、`already_subscribed`、`kernel`（message 为 `KernelError` 的文本）、`memory`（message 为 `MemoryError` 的文本）、`lagged`。

## 7. 客户端（`client.rs`、`chat.rs`）

`client.rs` 提供连接、发送请求并等待对应 id 的响应、读取推送。管理命令在 `main.rs` 中把 `data` 打印为格式化的 JSON。

`enco chat`：

1. 连接并订阅 Session（只显示订阅之后的新条目；历史用 `enco log` 查看）。
2. 读取标准输入的每一行：`/cancel` 发送 `cancel`；`/quit` 或 EOF 退出；其他内容作为 `send` 发出，`event_id` 由客户端生成。
3. 渲染推送的条目：

| 条目 | 显示 |
|---|---|
| `EventConsumed`（UserMessage） | 不显示（主人刚刚输入过） |
| `EventConsumed`（Reminder） | `⏰ {text}` |
| `RoundStarted`，`safe_mode` 为真（每个 Run 只提示一次） | `(safe mode)` |
| `AttemptSettled`（Completed，目的为 Reply，文本非空） | 文本 |
| `AttemptSettled`（Failed） | `! model request failed: {code}: {message}` |
| `ToolCallStarted` | `  → {capability.name}` |
| `ToolCallSettled` | `  ← ok` / `  ← failed: {code}` / `  ← unknown: {code}` |
| `Compacted` | `  (context compacted)` |
| `RunEnded`（非 Completed） | `! run ended: {kind}` |
| 其他 | 不显示 |

客户端通过之前看到的 `AttemptStarted` 记住每个 Attempt 的目的。

## 8. 日志

只有守护进程写日志：`tracing-subscriber` 输出到 stderr，过滤器取自 `ENCO_LOG` 环境变量，默认 `info`。客户端命令只向 stdout 输出结果。
