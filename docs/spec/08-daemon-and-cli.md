# 08 守护进程与 CLI（apps/enco）

`apps/enco` 是组合根：读取配置，构造全部适配器，启动 Kernel，并通过本地协议对外提供服务。CLI 的聊天与管理命令都是这个协议的客户端。它们与 Agent 的内置工具调用的是同一组 Kernel 方法（04 §3）。

## 1. 子命令

| 命令 | 作用 |
|---|---|
| `enco init` | 创建 `ENCO_HOME`、`.data/`、工作区目录、`.gitignore` 与配置模板（已存在的文件不覆盖），打印下一步 |
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
| `enco inspect [--session <name>] [--attempt <id>]` | 输出一次 Attempt 实际发出的请求（04 §14），默认最近一次 |
| `enco profile <session> <profile>` | 改变 Session 的 profile，下一个 Round 生效（12 §4） |
| `enco plugin status` | 每个插件的身份、活跃代际、回退目标、使用者与全部代际（11 §8） |
| `enco plugin deploy <name> <path>` | 把一个组件文件部署为 `<name>` 的新代际 |
| `enco plugin rollback <name>` | 回到上一个健康代际 |

除 `init` 与 `serve` 外，所有命令都连接正在运行的守护进程；连接失败时提示 `enco serve is not running`。管理命令不经过模型，模型不可用时照样可以使用。`enco plugin deploy` 把路径交给守护进程，由守护进程读取文件：两者在同一台机器上，不必把字节塞进协议。

管理命令把协议返回的 `data` 输出为格式化的 JSON，这是唯一的输出形式；验收测试直接解析它（09 §4）。

## 2. 路径（`paths.rs`）

`ENCO_HOME` 环境变量，默认为 `dirs::home_dir()/.enco`。

```text
$ENCO_HOME/            主人的意图；P2 起是一个 git 仓库（架构文档 §4.8）
  config.toml
  AGENTS.md            主人的常驻指令，可选（05 §1）
  plugins.lock         插件名字与身份，由守护进程维护（11 §2）
  plugins/<name>/      主人修改的插件源码；P1 不读它，构建由主人自己完成（11 §8）
  .gitignore           /.data/ 与 /workspace/
  workspace/           工具的默认工作目录，不纳入版本管理
  .data/               运行时状态，不纳入版本管理
    enco.db
    memory.db          记忆的权威（06 §2）
    memory-index/      记忆索引，派生物，随时可以删除（06 §3）
    blobs/
    artifacts/         制品库（03 §3.4）
    enco.sock
    enco.lock          单实例锁，永不删除（§5）
```

`Paths` 为每一项提供一个方法，其余代码不拼接路径。旧布局的开发数据不迁移，删除后重新 `enco init`。

## 3. 配置（`config.rs`）

`$ENCO_HOME/config.toml`，结构体全部 `#[serde(deny_unknown_fields)]`：

```toml
[endpoint.chat]                   # 至少一个；名字任取（12 §2）
plugin = "deepseek"               # 插件名，须有活跃代际并导出 completion（11 §6）
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_env = "DEEPSEEK_API_KEY"  # 可选；不需要认证的本地服务可以省略
options = {}                      # 可选；合并进请求体，例如 { temperature = 0.7 }
window_tokens = 128000            # 这个模型的上下文窗口
max_output_tokens = 8192          # 这个模型的输出上限，须小于窗口

[endpoint.cheap]
plugin = "openai-compatible"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
window_tokens = 128000
max_output_tokens = 4096

[profile.default]                 # 必填；新 Session 用它
reply = "chat"                    # 回复用的 endpoint
compaction = "cheap"              # 压缩用的 endpoint；可以与 reply 相同
requires_lifeline = true          # 可选，默认 true

[embedding]                       # 必填；记忆使用（06）
plugin = "openai-compatible"      # 可选，默认 openai-compatible；须导出 embedding
base_url = "https://api.openai.com/v1"
model = "text-embedding-3-small"
dimensions = 1536                 # 模型返回的向量维度；与现有索引不一致时，索引自动重建
api_key_env = "OPENAI_API_KEY"    # 可选
options = {}                      # 可选；合并进请求体，例如支持缩短维度的模型可写 { dimensions = 512 }

[run]                             # 可选
max_rounds = 24                   # 默认 24

[telegram]                        # 可选；存在时启用 Telegram 渠道（10）
token_env = "TELEGRAM_BOT_TOKEN"  # bot token 所在的环境变量
owner_user_id = 123456789         # 主人的 Telegram user ID
api_base = "https://api.telegram.org"   # 可选
```

- 配置文件不存在：报错并提示运行 `enco init`。
- 缺少 `[profile.default]` 或 `[embedding]`，profile 引用了不存在的 endpoint，endpoint 的窗口或输出上限不合法：报错并拒绝启动。插件名是否存在、是否导出所需接口，由注册表启动时检查（11 §6）。
- 给出了 `api_key_env` 但该环境变量未设置：报错并拒绝启动。API key 只从环境变量读取，不写进配置文件，读出后放在内存中的 `Endpoint.api_key`（12 §3）。`[telegram]` 的 `token_env` 同理。
- 修改配置要重启守护进程。
- 这是全部配置项。新增配置项需要先提问（01 §5）。

## 4. 组合根（`compose_root.rs`）

```text
paths  = Paths::from_env()
config = Config::load(paths.config())                               // profiles、embedding endpoint、run、telegram 都已校验（§3）
kernel_config = KernelConfig::new(config.run.max_rounds)?           // 在打开任何资源之前失败
确保 workspace/ 存在
clock  = Arc::new(SystemClock)
store  = SqliteStore::open(StorePaths { db: paths.db(), blobs: paths.blobs(), artifacts: paths.artifacts(), plugins_lock: paths.plugins_lock() })
runtime = Arc::new(WasmRuntime::new())                              // 07 §3
registry = Registry::open(RegistryDeps {                            // 11 §4.1：登记出厂制品、加载活跃代际、检查接线
    store, runtime,
    factory: FACTORY 中每一项的 (name, id, 嵌入的字节),               // 07 §5
    wiring: config.wiring(),                                        // 每个 profile 的两个用途 + embedding
    clock,
})
memories = Memories::open(MemoryPaths { db: paths.memory_db(), index: paths.memory_index() },
                          config.embedding_endpoint(), registry.clone(), clock.clone())   // 06 §7
deps = KernelDeps {
    store, registry, profiles: config.profiles(),
    composer: FactoryComposer::new(paths.workspace(), paths.instructions()),
    context:  vec![InstructionsContextSource::new(paths.instructions()), MemoryContextSource::new(memories.clone())],
    tools:    enco_host::native_tools(paths.workspace()) ++ enco_host::memory_tools(memories.clone())
              ++ enco_host::plugin_tools(registry.clone(), paths.workspace()),
    lifeline: enco_host::LIFELINE 转为 Vec<String>,
    clock,
}
kernel = Kernel::start(deps, kernel_config)
channels = config.telegram.map(|c| Channel::start(kernel.clone(), Telegram::new(token, api_base), owner_id, clock)) // 10；配置先校验，Kernel 启动后注册
```

出厂插件的名字与身份是组合根里的常量：

```rust
/// 出厂插件：名字由宿主保留，身份是固定的 ULID（11 §2），字节由 build.rs 嵌入（07 §5）。
const FACTORY: [(&str, &str, &[u8]); 2] = [
    ("openai-compatible", "01M3X4HYHSE2M3523YK35VX60W", OPENAI),
    ("deepseek",          "01M3X4HYHSRXVVQYXVDK5WQ9D3", DEEPSEEK),
];
```

两个 ULID 在第一次写进规格时定下，此后永不改变：它们就是这两个插件在任何 Space 里的身份。

依赖在这里一次性构造成具体的结构体。`Registry` 是代际的归属者，不是服务定位器：它只回答"这个名字现在是哪个代际"，不构造其他依赖。注册表先于 Kernel 打开，因为记忆在 Kernel 之前就需要嵌入导出。

## 5. 守护进程（`daemon.rs`）

启动：

1. 解析路径，打开 `enco.lock`（不存在则创建），用 `File::try_lock` 取得独占锁；锁已被占用说明已有守护进程在运行（包括正在关闭的），报错退出；其他打开或锁定错误携带上下文返回。锁由操作系统在进程退出时释放，`kill -9` 也不例外。锁文件永不删除：删掉它，新旧进程就会各锁一个文件。
2. 持有锁时，已有的 `enco.sock` 一定是残留文件，直接删除，不必尝试连接。绑定 Unix domain socket，并把文件权限设为 `0600`（这个 socket 等同于对 Agent 的完全控制权）。socket 只是通信入口，所有权由锁表示。
3. 执行组合根。锁在打开数据库与记忆索引、启动 Kernel 之前取得，所以第二次 `enco serve` 不会触发索引重建或 Session 恢复。组合根失败时删除本进程的 socket，返回错误。
4. 接受循环：每个连接一个任务。
5. 收到 SIGINT 或 SIGTERM：停止接受新连接，停止渠道并等待它静止（10 §6），调用 `kernel.shutdown()` 并等待它完成，删除 socket 文件，退出。锁一直持有到进程退出，所以关闭期间启动的 `enco serve` 会在第 1 步被拒绝。

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
    Inspect { session: String, attempt_id: Option<AttemptId> },
    SetProfile { session: String, profile: String },
    PluginStatus {},
    PluginDeploy { name: String, path: PathBuf },
    PluginRollback { name: String },
}
```

**命名规则**：裸 `id` 只属于请求信封，用来关联请求与响应；命令中引用某个对象的字段一律写作 `<种类>_id`，与已有的 `event_id` 一致。例如 `{"id":1,"cmd":"cancel_schedule","schedule_id":"<ULID>"}`。工具参数没有信封，仍然使用 `id`。

`deny_unknown_fields` 放在 `Command` 上；无参数命令使用空字段变体，使其同样拒绝多余字段。不要在带 `flatten` 的外层 `Request` 上再加该属性，否则正常的 `cmd` 也会被拒绝。JSON 形状仍为 `{"id":1,"cmd":"status"}`。

`send` 与 `subscribe` 在 Session 不存在时创建它（`Kernel::open_session`）；其余命令遇到不存在的 Session 返回 `unknown_session`。

`memories` 与 `forget_memory` 直接调用 `Memories::list` 与 `Memories::forget`，与 Agent 的记忆工具是同一组方法（06 §1）；`plugin_*` 调用 `kernel.registry()` 的方法，与 Agent 的插件工具是同一组方法（11 §8）；其余命令调用 Kernel。`plugin_deploy` 的 `path` 由 CLI 先解析为绝对路径再发送，守护进程读取文件，读不到时返回 `bad_request`。

### 6.2 服务端消息

```rust
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ok { id: u64, data: serde_json::Value },
    Error { id: u64, code: String, message: String },
    Entry { session: String, entry: Box<Entry> },   // 订阅之后推送
}
```

| 命令 | `data` |
|---|---|
| `send` | `{ "session_id", "event_id", "accepted": "new" \| "duplicate" }` |
| `subscribe` | `{ "session_id" }` |
| `cancel` | `{ "cancelled": bool }` |
| `status` | `Status`，外加 `channels`：`[{ "key", "running": bool, "stopped": string \| null, "recent_failures": [DeliverySettlement] }]`，由守护进程合并；各字段的含义见 10 §5 |
| `sessions` | `[SessionRecord]` |
| `log` | `[Entry]` |
| `safe_mode` | `{ "enabled": bool }` |
| `schedules` | `[Schedule]`（仅 Pending） |
| `cancel_schedule` | `{ "cancelled": bool }` |
| `memories` | `MemoryList`（06 §7） |
| `forget_memory` | `{ "forgotten": bool }` |
| `inspect` | `Inspection`（04 §14） |
| `set_profile` | `{ "profile": string }` |
| `plugin_status` | `[PluginStatus]`（11 §4） |
| `plugin_deploy` | `Deployed`（11 §4） |
| `plugin_rollback` | `GenerationRecord` |

错误码：`bad_request`（无法解析的请求，或 `plugin_deploy` 的文件读不到）、`unknown_session`、`already_subscribed`、`kernel`（message 为 `KernelError` 的文本，包括注册表与 profile 的错误）、`memory`（message 为 `MemoryError` 的文本）、`lagged`。

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
| `EventConsumed`（GenerationRolledBack） | `! plugin {plugin} rolled back: generation {from} failed ({code})` |
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
