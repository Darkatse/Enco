# 08 守护进程与 CLI（apps/enco）

`apps/enco` 是组合根：读取配置，构造全部适配器，启动 Kernel，并通过本地协议对外提供服务。CLI 的聊天与管理命令都是这个协议的客户端。它们与 Agent 的内置工具调用的是同一组 Kernel 方法（04 §3）。

## 1. 子命令

`enco --version` 打印版本，不连接守护进程。

| 命令 | 作用 |
|---|---|
| `enco init` | 创建 `ENCO_HOME`、`.data/`、工作区目录、`.gitignore` 与配置模板（已存在的文件不覆盖），打印下一步。模板的 `[owner] timezone` 填入检测到的本机 IANA 时区；检测不到时这一行写成注释，下一步提示中要求填写 |
| `enco serve` | 在前台运行守护进程 |
| `enco chat [--session <name>]` | 交互式对话，默认 Session 为 `main` |
| `enco send [--session <name>] [--wait] <text>` | 发送一条消息。不带 `--wait` 时发出即退出；带 `--wait` 时等这条输入所在的 Run 结束，输出它的结局（§7） |
| `enco status` | 节点、安全模式、代码版本、各 Session 状态、Scheduler 停止的原因（`scheduler_stopped`），以及配置：活跃配置代际与历史、当前宿主解析不了的表、`config.toml` 与它的不一致（§3.2） |
| `enco sessions` | 列出 Session |
| `enco log [--session <name>]` | 输出 Session 的全部条目 |
| `enco safe-mode <on\|off>` | 切换安全模式（下一个 Round 生效） |
| `enco schedules [--cancel <id>]` | 列出 Active 的定时（规则、下一次触发与最近一次触发），或取消一个 |
| `enco memory [--forget <id>]` | 列出全部记忆（标出置顶与尚未进入索引的），或遗忘一条 |
| `enco cancel [--session <name>]` | 取消正在进行的 Run |
| `enco inspect [--session <name>] [--attempt <id>]` | 输出一次 Attempt 实际发出的请求（04 §14），默认最近一次 |
| `enco profile <session> <profile>` | 改变 Session 的 profile，下一个 Round 生效（12 §4） |
| `enco plugin status` | 每个插件的身份、活跃代际、回退目标、使用者与全部代际（11 §8） |
| `enco plugin deploy <name> (<path> \| --generation <N>)` | 把一个组件文件部署为 `<name>` 的新代际，或重新部署某个已有代际的制品 |
| `enco plugin rollback <name>` | 回到回退目标。机械恢复，不经校验，只在 CLI 中（11 §8） |
| `enco config apply [--generation <N>]` | 应用 `config.toml`，或重新登记某个已有配置代际的文档（§3.1） |
| `enco config rollback` | 回到上一个配置代际。机械恢复，不经校验，只在 CLI 中（11 §5） |

除 `init` 与 `serve` 外，所有命令都连接正在运行的守护进程；连接失败时提示 `enco serve is not running`。管理命令不经过模型，模型不可用时照样可以使用。`enco plugin deploy` 把路径交给守护进程，由守护进程读取文件：两者在同一台机器上，不必把字节塞进协议。

管理命令把协议返回的 `data` 输出为格式化的 JSON，这是唯一的输出形式；验收测试直接解析它。

## 2. 路径（`paths.rs`）

`ENCO_HOME` 环境变量，默认为 `dirs::home_dir()/.enco`。

```text
$ENCO_HOME/            主人的意图；P2 起是一个 git 仓库（架构文档 §4.8）
  config.toml
  AGENTS.md            主人的常驻指令，可选（05 §1）
  plugins.lock         插件名字与身份，由守护进程维护（11 §2）
  plugins/<name>/      主人修改的插件源码；注册表不读它，构建由主人自己完成（11 §8）
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

`Paths` 为每一项提供一个方法，其余代码不拼接路径。

## 3. 配置

`$ENCO_HOME/config.toml`。每张表由它的归属者解析（架构文档 §4.11），都不接受未知字段：

```toml
[owner]                           # 关于主人的事实（12 §2）
timezone = "America/New_York"     # 必填；IANA 名称。时间显示与跟随型周期定时都用它（04 §10、05 §4）

[run]                             # 可选
max_rounds = 24                   # 默认 24

[endpoint.chat]                   # 模型服务；名字任取，profile 与 [memory] 按名字引用（12 §2）
plugin = "deepseek"               # 插件名，须有活跃代际并导出所需接口（11 §6）
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_env = "DEEPSEEK_API_KEY"  # 插件声明凭证为必需时必填；不需要认证的本地服务可以省略（11 §6）
options = {}                      # 可选；合并进请求体，例如 { temperature = 0.7 }
window_tokens = 128000            # 回复与压缩用的 endpoint 必填：这个模型的上下文窗口
max_output_tokens = 8192          # 回复与压缩用的 endpoint 必填：输出上限，须小于窗口

[endpoint.cheap]
plugin = "openai-compatible"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
window_tokens = 128000
max_output_tokens = 4096

[endpoint.embed]
plugin = "openai-compatible"
base_url = "https://api.openai.com/v1"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536                 # 记忆的 embedding 所用的 endpoint 必填（06 §3.2）
options = {}                      # 支持缩短维度的模型可写 { dimensions = 512 }

[endpoint.relevance]
plugin = "typesafe"
base_url = "https://api.typesafe.ai/v1"
model = "jev-latest"
api_key_env = "TYPESAFE_API_KEY"

[profile.default]                 # 必填；新 Session 用它
reply = "chat"                    # 回复用的 endpoint
compaction = "cheap"              # 压缩用的 endpoint；可以与 reply 相同
requires_lifeline = true          # 可选，默认 true

[memory]                          # 必填（06 §7）
embedding = "embed"               # 检索索引用的 endpoint
relevance = "relevance"           # 可选；写了时自动召回的记忆按相关性过滤（06 §5.1）

[telegram]                        # 可选；存在时启用 Telegram 渠道（10）
token_env = "TELEGRAM_BOT_TOKEN"  # bot token 所在的环境变量
owner_user_id = 123456789         # 主人的 Telegram user ID
api_base = "https://api.telegram.org"   # 可选
```

- 各表的含义见它的归属者：内核（12 §2）、记忆（06 §7）、Telegram 渠道（10 §1）。插件的 `[plugin.*]` 随第一个有配置的插件加入（路线图 M23）。
- **修改之后执行应用配置**（§3.1）。重启只恢复活跃配置代际，不读取 `config.toml` 的新内容（11 §4.1）。
- **配置文件不存在或无效**：全新的 home 拒绝启动并提示运行 `enco init`；已经有配置代际的 home 照常启动，只给出警告（§3.2）。
- **环境变量只记名字**，值在用到时读取（07 §3.3、10 §1），启动与应用配置都不检查它们。
- 这是全部配置项。新增配置项需要先提问（01 §5）。

### 3.1 应用配置（`enco-host` 的 `tools/config.rs`）

CLI 的 `enco config apply` 与工具 `config_apply`（05 §2.6）调用同一个函数：

```text
document(path):  读 config.toml，转成 JSON，保留全部的表；读不到或不是合法的 TOML 时返回 ConfigFileError
apply(path, registry, generation):
    source = generation 给出时为 ConfigSource::Generation(N)，否则为 ConfigSource::Document(document(path)?)
    registry.apply_config(source)                    // 11 §5
    return { generation, previous, unchanged, probes }
```

宿主只读文件，不解析任何一张表。以编号应用就是撤销（架构文档 §4.6）。首次登记也用 `document(path)`，作为 `RegistryDeps.bootstrap` 交给注册表（11 §4.1）。

### 3.2 不一致

守护进程每次启动时、以及 `enco status` 时，把 `document(path)` 的结果交给 `Registry::config_divergence`（11 §4.3）；启动日志对 `superseded` 与 `unapplied` 给出警告。

## 4. 组合根（`compose_root.rs`）

```text
paths  = Paths::from_env()
确保 workspace/ 存在
clock  = Arc::new(SystemClock)
store  = SqliteStore::open(StorePaths { db: paths.db(), blobs: paths.blobs(), artifacts: paths.artifacts(), plugins_lock: paths.plugins_lock() })
runtime = Arc::new(WasmRuntime::new())                              // 07 §3
registry = Registry::open(RegistryDeps {                            // 11 §4.1：登记出厂制品、加载活跃代际、还原或首次登记配置
    store, runtime,
    factory: FACTORY 中每一项的 (name, id, 嵌入的字节),               // 07 §5
    owners: enco_host::TABLE_OWNERS.into(),   // 宿主一侧的归属者（06 §7、10 §1）
    bootstrap: enco_host::config::document(&paths.config()).await.map_err(|error| error.to_string()),                            // §3.1；只在还没有配置代际时使用
    clock,
})
按 §3.2 报告 config.toml 与活跃配置代际的不一致
memories = Memories::open(MemoryPaths { db: paths.memory_db(), index: paths.memory_index() }, registry.clone(), clock.clone())   // 06 §7：[memory] 每次对账时从视图读取
deps = KernelDeps {
    store, registry,
    composer: FactoryComposer::new(paths.workspace(), paths.instructions()),
    context:  vec![InstructionsContextSource::new(paths.instructions()),
                   MemoryContextSource::new(memories.clone())],   // 06 §5
    tools:    enco_host::native_tools(paths.workspace()) ++ enco_host::memory_tools(memories.clone())
              ++ enco_host::plugin_tools(registry.clone(), paths.workspace()) ++ enco_host::config::tools(paths.config(), registry.clone()),   // config_apply 调用 §3.1
    lifeline: enco_host::LIFELINE 转为 Vec<String>,
    clock,
}
kernel = Kernel::start(deps)
channels = 启动渠道的监督任务（10 §6）
```

出厂插件的名字与身份是组合根里的常量：

```rust
/// 出厂插件：名字由宿主保留，身份是固定的 ULID（11 §2），字节由 build.rs 嵌入（07 §5）。
const FACTORY: [(&str, &str, &[u8]); 3] = [
    ("openai-compatible", "01M3X4HYHSE2M3523YK35VX60W", OPENAI),
    ("deepseek",          "01M3X4HYHSRXVVQYXVDK5WQ9D3", DEEPSEEK),
    ("typesafe",          "01M4EQVYF70Z6WGYFDKD20QF8S", TYPESAFE),
];
```

每个 ULID 在第一次写进规格时定下，此后永不改变：它就是这个插件在任何 Space 里的身份。

依赖在这里一次性构造成具体的结构体。`Registry` 是代际的归属者，不是服务定位器：它只回答"这个名字现在是哪个代际"与"活跃配置是什么"，不构造其他依赖。注册表最先打开，因为各组件都从它发布的视图读取自己的表，记忆在 Kernel 之前还需要嵌入导出。组合根不解析配置，只决定先后。

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
    PluginDeploy { name: String, source: DeploySource }, // Path(PathBuf) 或 Generation(GenerationId)
    PluginRollback { name: String },
    ConfigApply { generation: Option<GenerationId> },   // 给出时重新登记这个配置代际的文档
    ConfigRollback {},
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploySource { Path(PathBuf), Generation(GenerationId) }
```

**命名规则**：裸 `id` 只属于请求信封，用来关联请求与响应；命令中引用某个对象的字段一律写作 `<种类>_id`，与已有的 `event_id` 一致。例如 `{"id":1,"cmd":"cancel_schedule","schedule_id":"<ULID>"}`。工具参数没有信封，仍然使用 `id`。

`deny_unknown_fields` 放在 `Command` 上；无参数命令使用空字段变体，使其同样拒绝多余字段。不要在带 `flatten` 的外层 `Request` 上再加该属性，否则正常的 `cmd` 也会被拒绝。JSON 形状仍为 `{"id":1,"cmd":"status"}`。

`send` 与 `subscribe` 在 Session 不存在时创建它（`Kernel::open_session`）；其余命令遇到不存在的 Session 返回 `unknown_session`。

`memories` 与 `forget_memory` 直接调用 `Memories::list` 与 `Memories::forget`，与 Agent 的记忆工具是同一组方法（06 §1）；`plugin_*` 调用 `kernel.registry()` 的方法，与 Agent 的插件工具是同一组方法（11 §8）；`config_apply` 调用 §3.1 的函数，与工具 `config_apply` 相同；`config_rollback` 调用 `Registry::rollback_config`；其余命令调用 Kernel。`plugin_deploy` 的 `source` 是 `{"path": "..."}` 或 `{"generation": N}`；路径由 CLI 先解析为绝对路径再发送，守护进程读取文件，读不到时返回 `bad_request`。

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
| `status` | `Status`，外加由守护进程合并的两项：`channels`：`[{ "key", "running": bool, "stopped": string \| null, "recent_failures": [DeliverySettlement] }]`，各字段的含义见 10 §5；`config`：`ConfigStatus`（11 §4，`invalid` 列出当前宿主解析不了的表）加上 `divergence`（`none`、`superseded`、`unapplied`，§3.2） |
| `sessions` | `[SessionRecord]` |
| `log` | `[Entry]` |
| `safe_mode` | `{ "enabled": bool }` |
| `schedules` | 与 `schedule_list` 共用的查询视图（04 §10–§11），列出所有 Session 的 Active 定时；时刻按各定时的生效时区显示 |
| `cancel_schedule` | `{ "cancelled": bool }` |
| `memories` | `MemoryList`（06 §7） |
| `forget_memory` | `{ "forgotten": bool }` |
| `inspect` | `Inspection`（04 §14） |
| `set_profile` | `{ "profile": string }` |
| `plugin_status` | `[PluginStatus]`（11 §4） |
| `plugin_deploy` | `Deployed`（11 §4） |
| `plugin_rollback` | `GenerationRecord` |
| `config_apply` | `{ "generation", "previous", "unchanged": bool, "probes": [Probe] }`（§3.1；`Probe` 见 11 §4） |
| `config_rollback` | `{ "generation" }` |

错误码：`bad_request`（无法解析的请求，`plugin_deploy` 的文件读不到，或者 `path` 与 `generation` 没有恰好给一个）、`config`（message 为应用配置被拒绝的原因，§3.1）、`unknown_session`、`already_subscribed`、`kernel`（message 为 `KernelError` 的文本，包括注册表与 profile 的错误）、`memory`（message 为 `MemoryError` 的文本）、`lagged`。

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

`enco send --wait` 只用已有的 `subscribe`、`send` 与 `log`，协议不变：

1. 先 `subscribe` 这个 Session，再 `send`，所以接纳这条输入的条目一定在订阅之后。
2. 在推送中找 `event.id` 等于本次 `event_id` 的 `EventConsumed`，然后取它之后的第一个 `RunEnded`。"所在的 Run"由 Log 认定：输入落进一个已在进行的 Run 时，结局就是这个 Run 的结局。
3. 收到 `lagged` 时，用 `log { after }` 从最后看到的位置补读。守护进程按顺序写出同一连接的回复与推送，所以补读回复之前到达的推送与 `lagged` 都已包含在回复中，直接丢弃；回复之后的推送可能与回复重叠，按 Log 位置去重。
4. 输出 `{ "session_id", "event_id", "accepted", "run", "end" }`，`end` 是 `RunEnd`，然后退出。等待没有时限，需要时限的调用方自己计时并 `enco cancel`（13 §4）。

## 8. 日志

只有守护进程写日志：`tracing-subscriber` 输出到 stderr，过滤器取自 `ENCO_LOG` 环境变量，默认 `info`。客户端命令只向 stdout 输出结果。
