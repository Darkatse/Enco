# 07 插件边界：WIT、enco-wasm 与 Provider 插件

OpenAI-Compatible、DeepSeek 与 TypeSafe 三个 Provider 插件共享 `plugins/provider-protocol` 中的 WIT 绑定、HTTP 传输与失败分类。它们都以 Wasm 组件运行，作为出厂代际嵌入宿主二进制（11 §3）。插件边界从第一天起真实存在：插件只看到 WIT，宿主只看到端口。

本章描述当前契约 `enco:plugin@0.3.0`。工具接口与插件自带的凭证随第一个工具插件加入，渠道接口在 P3，状态读取等到第一个有状态的插件。契约不预留：每一项随第一个使用者加入，需要破坏就升主版本（架构文档 §4.4）。

## 1. WIT（`wit/`）

包 `enco:plugin@0.3.0`。契约只有一份来源：[`wit/plugin.wit`](../../wit/plugin.wit)，它的文档注释生成 [`wit/CONTRACT.md`](../../wit/CONTRACT.md)（§2）。规格不另存 WIT 副本。

消息与工具调用是协议本身，用 WIT 类型表达；只有工具的参数与 schema 这类业务内容是 JSON 字符串。

| 条目 | 内容 |
|---|---|
| `types` | `json`、`failure`、`role`、`tool-call`、`tool-result`、`extension { data }`、`part`、`message`、`tool-spec`、`settings { base-url, model, options: json }` |
| `host` | 宿主导入：`log`，以及异步的 `http`（失败带 `request-sent`；宿主附加凭证、只在同一 origin 内跟随重定向、遮蔽密钥的值，§3.2） |
| `completion` | `request`、`usage`、`stop-reason`、`completion`，以及异步的 `complete(settings, request)` |
| `embedding` | 异步的 `embed(settings, inputs)` |
| `decision` | `label`、`question-kind`（`predicate`、`choice`、`score`）、`question`、`answer`（另有 `refused`），以及异步的 `decide(settings, state, questions)` |
| `lifecycle` | `credential { header, prefix, required }`；`description { summary, config-schema: json, credential: option<credential> }`；`describe(config) -> result<description, failure>`；异步的 `probe() -> result<_, failure>` |
| `base` / `completion-plugin` / `embedding-plugin` / `decision-plugin` / `provider-plugin` | `base` 导入 `host`、导出 `lifecycle`；其余在其上分别导出 `completion`、`embedding`、`decision`，`provider-plugin` 导出前两者 |

契约按使用者划分，声明只包含实际需要的事实：

- **`completion`、`embedding` 与 `decision` 分开导出。** 它们的导入方与约束不同，也可以单独提供：DeepSeek 只导出补全，TypeSafe 只导出判断。调用参数由 `types.settings` 共用；需要多个导出的插件通过 `include` 组合 world。
- **身份由注册表拥有。** `describe(config)` 返回 `summary`、`config-schema` 与 `credential`；配置无法接受时返回 `failure`。名字与身份在 `plugins.lock` 中。扩展字段的归属由产生它的 Attempt 所记录的代际确定，内核只向同一插件身份回放（04 §6.5）。
- **`probe` 只做自检。** 它不联网，也不依赖主人的配置；部署时调用，失败即拒绝（11 §7）。
- **`decision` 返回概率。** 对同一份文本 state 独立判断每个问题，按问题顺序返回概率分布或 `refused`；选中项、分数与后续政策由导入方决定。
- **密钥只出现在传输层。** 插件声明请求头、前缀与是否必需；宿主在调用时按 endpoint 的 `api_key_env` 读取值，仅附加到 `base-url` 所在 origin 的请求上（§3.2）。插件、内核与 Log 都不接触值。
- **凭证至多一个。** `option<credential>` 表达当前服务所需的凭证；插件自带的凭证及其他放置方式随第一个使用者加入。
- **宿主只链接当前契约的主版本。** 不匹配的制品在加载时被拒绝，错误指出版本；插件源码归主人，出厂插件随宿主一起重建。

每个 world、interface、type、function 都必须有英文的 `///` 文档（02 §7），09 §2 的门禁会检查。失败码是普通字符串，与内核使用的码（03 §1.6）完全相同，不做任何转换。

流式补全随第一个消费者加入。给已有的导出接口加函数是破坏性变化（宿主会检查组件导出的每个函数），届时升主版本，`complete` 不变。

## 2. CONTRACT.md

`wit/CONTRACT.md` 由 `cargo xtask docs` 生成（09 §2），不得手改。

## 3. enco-wasm

enco-wasm 实现内核的五个插件端口（04 §2）：`WasmRuntime` 实现 `Runtime`，`WasmPlugin` 实现 `Lifecycle`、`Provider`、`Embedding`、`Decision`。它不认识注册表，也不认识代际：给它制品与配置，它返回导出。

### 3.1 Engine（`engine.rs`）

```rust
pub struct WasmEngine { engine: wasmtime::Engine, ticker: JoinHandle<()> }
```

- `Config`：`wasm_component_model_async(true)`、`epoch_interruption(true)`；其余用默认值（Cranelift）。
- 启动一个 epoch 计时器任务：每 `EPOCH_TICK`（10 ms）调用一次 `engine.increment_epoch()`。这是 enco-wasm 唯一的后台任务，在 `WasmEngine` 被 drop 时 abort。
- 每次加载都从字节编译组件（原型实测约 27 ms），**不使用** `Component::deserialize`，因此不需要 unsafe。

### 3.2 宿主导入（`host_imports.rs`）

- `log`：转发到 `tracing`，target 为 `plugin`，附带插件的制品哈希前八位。
- `http`：使用一个共享的 `reqwest::Client`（rustls）。
  - 超时：请求中的 `timeout-ms`，否则 `HTTP_DEFAULT_TIMEOUT`（300 s）。
  - 响应体上限 `HTTP_MAX_BODY_BYTES`（32 MiB），超出返回 `too-large`。
  - 失败分类：URL 或方法无效 → `invalid-request`，`request-sent = false`；连接失败 → `connect`，`request-sent = false`；超时 → `timeout`，`request-sent = true`；读取响应体失败 → `body`，`request-sent = true`；其他 → `other`，`request-sent = true`。
- **凭证**：每次调用的 `HostState` 带着这次调用的绑定（§3.3），每个绑定是 (origin, 请求头名, 前缀, 值, 变量名)。
  - 附加：请求 URL 的 origin（scheme、host、port）等于某个绑定的 origin 时，设置请求头 `<请求头名>: <前缀><值>`，覆盖插件自己写的同名请求头。`http-request` 不变。
  - 重定向：共享的 `reqwest::Client` 使用自定义的重定向策略，只跟随同一 origin 内的重定向（至多 `HTTP_MAX_REDIRECTS` 跳），附加的请求头随之保留；跨 origin 的 3xx 原样作为响应交回插件。reqwest 默认跨 host 时只剥离 `Authorization` 等少数请求头，`x-api-key` 这类请求头会被带走，所以由策略在源头停下。
  - 遮蔽：交回插件的一切，即响应头的值、响应体、失败消息，出现绑定的值时替换为 `[redacted:<变量名>]`。变量名本来就记在 Attempt 中，不是秘密。失败消息照旧用 `without_url()`。经过编码的形式（URL 编码、base64）不在替换之列。
- 不在任何日志中记录请求头与注入之后的 URL。

### 3.3 WasmRuntime 与 WasmPlugin（`runtime.rs`、`plugin.rs`）

```rust
pub struct WasmRuntime { engine: Arc<WasmEngine>, http: reqwest::Client }

impl WasmRuntime {
    pub fn new() -> Result<Self, WasmError>;
}

#[async_trait]
impl Runtime for WasmRuntime {
    async fn load(&self, artifact: &[u8], config: &serde_json::Value) -> Result<Loaded, LoadError>;
}
```

`load` 按顺序做四件事，任何一步失败都返回 `LoadError`，message 带 wasmtime 的原文：

1. `Component::new`。不是组件、格式不对，在这里失败。接着检查组件导入的 `enco:plugin/host` 的版本：主版本（`0.x` 中的 `x`）与宿主的不同时失败，message 形如 `built against enco:plugin@0.2.1; this host links enco:plugin@0.3.0, rebuild the plugin`，不把 wasmtime 的链接错误原样交给主人。
2. 构造 `Linker`（`wasmtime_wasi::p2::add_to_linker_async` 加上 bindgen 生成的宿主导入），链接一次得到未类型化的 `InstancePre`。导入不满足（WIT 版本不匹配、要了宿主没有的导入）在这里失败。
3. 用 `get_export_index(None, "enco:plugin/<interface>@0.3.0")` 查询 `completion`、`embedding` 与 `decision`，由 wasmtime 处理兼容的补丁版本。把上一步的 `InstancePre` 转为必需的 `BasePre`，以及实际导出所需的 `CompletionPluginPre`、`EmbeddingPluginPre`、`DecisionPluginPre` 视图；三种导出一个都没有也是失败：这份制品对 Enco 没有用处。
4. 实例化一次，调用 `describe(config)`。返回 `failure` 即失败。返回的凭证交给 `Loaded.credential`（只含是否必需，供接线检查，04 §2），配置 schema 解析为 JSON 后保存在 `Loaded.config_schema` 中，供手册使用，不再做一份配置校验；请求头与前缀经 HTTP 类型检查后留在 `WasmPlugin` 中。

bindgen 生成四个 world 的绑定（`base`、`completion-plugin`、`embedding-plugin`、`decision-plugin`），各自只要求自己的导出，通过 `with` 共享同一份 `types` 与 `host`。它们从同一个 `InstancePre` 建立视图；组件多出来的导出不影响实例化。

`Loaded` 的适配器共享同一个 `Arc<WasmPlugin>`。`describe` 与 `probe` 都经 `lifecycle`（`BasePre`）调用：

```rust
pub struct WasmPlugin {
    engine: Arc<WasmEngine>,
    http: reqwest::Client,
    name: String,                          // 制品哈希前八位，只用于日志
    lifecycle: BasePre<HostState>,
    completion: Option<CompletionPluginPre<HostState>>,
    embedding: Option<EmbeddingPluginPre<HostState>>,
    decision: Option<DecisionPluginPre<HostState>>,
    placement: Option<Placement>,          // describe 声明的请求头与前缀；是否必需交给 Loaded.credential
}
```

**每次调用一个新的 Store**（架构文档 §4.5 的"调用"）。`complete`、`embed`、`decide`、`probe` 走同一条调用路径：

- `HostState` 包含一个最小的 `WasiCtx`（只继承 stderr，没有预开放目录、没有环境变量、没有参数）、`ResourceTable`、`reqwest::Client` 的克隆、制品哈希。
- `StoreLimits`：线性内存上限 `WASM_MEMORY_LIMIT`（256 MiB）。
- `store.epoch_deadline_async_yield_and_update(1)`：客户代码每个 epoch 让出一次，使外层的超时与取消能够生效。
- 用 `tokio::time::timeout(PLUGIN_CALL_TIMEOUT, …)`（330 s）包住整个调用；超时返回 `Failure { code: "timeout", retryable: true }`。
- 调用通过 `store.run_concurrent(async |accessor| …)` 进行（异步导出需要它）。
- `settings` 原样转为 WIT 的 `settings` 记录，其中没有密钥。插件声明了凭证、`settings.api_key_env` 给出了变量名时，读取这个环境变量，与 `base_url` 的 origin、声明的请求头与前缀组成这次调用的绑定，放进 `HostState`。变量未设置、为空或不能作为请求头值时返回 `Failure { code: "credential.unavailable", retryable: false }`，不调用插件。插件没有声明凭证，或者 endpoint 没有给出变量名，就不附加。两者是否相符只在准入时检查（11 §6）：回退与安全模式之后可能不相符，调用照常进行，由服务给出结果，恢复路径上不再多一道拒绝。
- Store 随调用结束而丢弃。

**失败码的归类**只在这一层做一次，健康门控据此判断（11 §7）：

| 情况 | code | retryable |
|---|---|---|
| wasmtime 报错：trap、实例化失败、内存超限、导出函数不存在 | `plugin.trap` | false |
| 返回值违反契约：`completion.message.role` 不是 assistant、`extension.data` 不是合法 JSON、向量数量与输入不符、向量为空、维度彼此不同或含非有限数、决策答案违反下文的规则 | `plugin.contract` | false |
| 超时 | `timeout` | true |
| 凭证的环境变量缺失或值不可用 | `credential.unavailable` | false |
| 插件自己返回的 `failure` | 原样 | 原样 |

`convert.rs` 负责 enco-core 与 WIT 类型之间的双向转换：

- 请求：`ToolCall` → `tool-call { id: provider_id, … }`；`ToolResult` → `tool-result { call-id: provider_id, … }`；`Extension { data }` → `extension { data: 序列化的 JSON }`。
- 结果：每个 `tool-call` 生成一个新的 `CallId`，`provider_id` 取自 `tool-call.id`；`extension.data` 解析为 JSON 值，失败即 `plugin.contract`。

嵌入的通用返回契约只在这个边界检查；记忆归属者只核对配置要求的目标维度（06 §3.3），配置不匹配不归因于插件代际。

决策的返回契约同样只在这里检查：答案数等于问题数；每个答案与对应问题同形，或者是 `refused`；概率有限，且在 [0, 1] 内；`choice` 与 `score` 的概率条数等于 label 数，总和与 1 相差不超过 `DECISION_SUM_TOLERANCE`。

原型中验证过的 wasmtime 49 细节：`wasmtime::Error` 是独立的类型，不是 `anyhow::Error`；bindgen 为异步导入生成 `HostWithStore<U>` trait，实现在 `HasSelf<HostState>` 上；`list` 是 WIT 关键字。

### 3.4 常量（`limits.rs`）

```rust
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
pub const WASM_MEMORY_LIMIT: usize = 256 * 1024 * 1024;
pub const PLUGIN_CALL_TIMEOUT: Duration = Duration::from_secs(330);
pub const HTTP_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
pub const HTTP_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
pub const HTTP_MAX_REDIRECTS: usize = 10;              // 只计同一 origin 内的跳转
pub const DECISION_SUM_TOLERANCE: f64 = 0.01;          // 契约（WIT `answer`）规定的值
```

## 4. Provider 插件（`plugins/`）

目录名就是插件名（架构文档 §4.10）：`plugins/openai-compatible/`（包名 `openai-compatible`，world `provider-plugin`）、`plugins/deepseek/`（包名 `deepseek`，world `completion-plugin`）与 `plugins/typesafe/`（包名 `typesafe`，world `decision-plugin`）。三个薄 `cdylib` 入口复用 `provider-protocol` 的类型与失败分类，前两个还复用其中的 Chat Completions 转换。

绑定的划分只有一条规则：world 属于插件，类型属于共享 crate。每个插件在自己的 crate 里 `generate!` 自己的 world，用 bindgen 的 `with` 把 `host`、`types`（Chat Completions 插件还有 `completion` 的四个类型）指向 `provider-protocol`。共享 crate 以库模式（`pub_export_macro: true`）生成 `completion-plugin` 绑定，提供类型与宿主导入；导出元数据只在插件入口展开自己的 `export!` 时产生，因此仅依赖共享类型不会要求插件导出 completion。每个 crate 只生成一个 world。导出宏展开出的 canonical ABI 胶水含 unsafe，生成它的模块写 `#![expect(unsafe_code, reason = …)]`（02 §5）。

插件里不再有名字常量。DeepSeek 插件目前没有专有逻辑，它存在的作用是一个独立的身份：它产生的扩展字段（例如 `reasoning_content`）只会回放给它自己（04 §6.5）。

### 4.1 请求

`POST {base-url}/chat/completions`，请求头 `Content-Type: application/json`。鉴权头由宿主按 `describe` 声明的凭证附加（§3.2、§4.6），插件不写。请求体：

```json
{
  "model": "<settings.model>",
  "messages": [...],
  "tools": [ { "type": "function", "function": { "name": "...", "description": "...", "parameters": <input-schema> } } ],
  "max_tokens": <max-output-tokens>
}
```

- `tools` 为空时省略该字段；`max-output-tokens` 为空时省略 `max_tokens`。
- `settings.options` 中的对象键逐一合并到请求体顶层（例如 `temperature`）。属于规范请求的保留键（model、messages、tools、输出上限、stream）不能通过 options 覆盖，即使这次请求省略了该字段；冲突返回 `provider.bad_request`。
- 消息映射：
  - system / user → `{ "role", "content": <拼接的文本> }`
  - assistant → `{ "role": "assistant", "content": <文本>, "tool_calls": [ { "id", "type": "function", "function": { "name", "arguments" } } ] }`（没有工具调用时省略 `tool_calls`）。协议只允许 `content` 在带工具调用时为 null，所以只有这种情况下空文本写作 null，否则写空串。消息中每个 `extension` 的字段合并回这条消息对象。宿主保证这些扩展字段是本插件产生的，插件不再过滤。
  - tool → `{ "role": "tool", "tool_call_id": <call-id>, "content" }`

### 4.2 响应

从 `choices[0].message` 读取：

- `content`（字符串，可能为 null）→ `text` 部分。
- `tool_calls` → `tool-call` 部分，`arguments` 原样保留为字符串。
- 除 `role`、`content`、`tool_calls` 之外的字段（例如某些服务商的 `reasoning_content`）→ 一个 `extension { data: {这些字段} }`，下次请求时合并回去（§4.1）。
- `finish_reason`：`stop` → `end-turn`，`tool_calls` → `tool-calls`，`length` → `max-tokens`，其他字符串 → `other`。缺失或不是字符串时返回 `provider.bad_response`：被上游掐断的响应（例如被内容审核截断）可能不带它，插件不猜测结局。
- `usage.prompt_tokens`、`usage.completion_tokens`、`usage.prompt_tokens_details.cached_tokens`（可能不存在）→ `usage`。

### 4.3 嵌入（`embed`，仅 openai-compatible）

`POST {base-url}/embeddings`，请求头同 §4.1，`http-request.timeout-ms` 为 30000。请求体：

```json
{ "model": "<settings.model>", "input": ["...", "..."] }
```

- `settings.options` 按 §4.1 的规则合并（例如 `{ "dimensions": 512 }`）。
- 响应：`data` 中每一项的 `embedding`（数字数组）按 `index` 排序后返回。兼容接口可以省略值为零的 `index`，插件统一解码为 0；之后仍校验位置覆盖完整且没有重复，绝不按任意缺项猜测顺序。`data` 的数量与 `inputs` 不同，或者缺少 `embedding`，返回 `provider.bad_response`。
- 维度由宿主核对（06 §3.3），插件不关心。

### 4.4 决策（`decide`，仅 typesafe）

`POST {base-url}/systemone`，请求头同 §4.1，`http-request.timeout-ms` 为 10000。请求体：

```json
{
  "model": "<settings.model>",
  "state": "<state>",
  "questions": {
    "q0": { "type": "noul", "instructions": "..." },
    "q1": { "type": "choice", "instructions": "...", "criteria": { "<name>": "<description>" } },
    "q2": { "type": "score", "instructions": "...", "criteria": ["<name>: <description>"] }
  }
}
```

- 键是 `q` 加问题的下标，只用来对应答案；TypeSafe 不把键交给模型。
- `choice` 的 label 没有描述时，值为 null。`score` 的等级没有名字，所以名字与描述拼成一段文本，没有描述时只用名字。
- `settings.options` 按 §4.1 的规则合并，保留键是 model、state、questions。
- 响应：按键取回 `answers` 中的每个答案，按问题顺序返回。`noul` → `predicate`。`choice` 的 `probabilities` 按 label 名取出，排成 label 顺序。`score` 的 `probabilities` 以等级在 criteria 中的位置为键（从 `"0"` 起），按编号排成 label 顺序。
- 服务的答案不满足 §3.3 的返回契约时，返回 `provider.bad_response`：服务的错误由插件拦下，宿主的契约检查只针对插件自身的错误。
- TypeSafe 没有拒绝回答的概念，这个插件不返回 `refused`。

### 4.5 失败分类

| 情况 | code | retryable |
|---|---|---|
| `http-failure`（任何类别） | `provider.network`；类别为 `timeout` 时为 `timeout` | true |
| 状态 401、403 | `provider.auth` | false |
| 状态 429 | `provider.rate_limited` | true |
| 状态 5xx | `provider.server` | true |
| 其他非 2xx | `provider.bad_request` | false |
| 响应不是合法 JSON，或不符合 §4.2 的映射（嵌入与决策见 §4.3、§4.4） | `provider.bad_response` | false |

非 2xx 时，`message` 包含状态码和响应体的前 500 字节；其中的密钥已由宿主遮蔽（§3.2），插件不再自己遮蔽。

模型请求没有需要结算的外部效果（至多重复计费），所以网络失败一律可以重试。是否重试由调用方决定：内核只重试 `complete`（04 §6.4），记忆不重试 `embed` 与 `decide`（06 §3.3）。这些都是外部失败，不计入健康（11 §7）。

### 4.6 lifecycle

- `describe(config)`：这些插件没有配置项。配置是空对象时返回 `description`，否则返回 `failure`：
  - `summary`：一句话说明它接什么协议，例如 `OpenAI-compatible Chat Completions and Embeddings`；
  - `config-schema`：`{"type":"object","additionalProperties":false}`；
  - `credential`：`authorization` 请求头，前缀 `Bearer `。openai-compatible 声明为可选，因为不需要认证的本地服务也走它；deepseek 与 typesafe 声明为必需。
- `probe()`：返回成功。这些插件没有不联网就能检查的东西；probe 的作用是证明制品能实例化、能调用导出。

## 5. 出厂代际的嵌入

1. `cargo xtask build-factory` 构建整个插件工作区（`--workspace --target wasm32-wasip2 --release`），把每个出厂插件的产物复制为 `target/factory/<插件名>.wasm`。
2. `apps/enco/build.rs` 检查每份制品存在；缺失则提示先运行 `cargo xtask build-factory`。为每份制品输出 `cargo:rerun-if-changed` 和绝对路径环境变量（`ENCO_FACTORY_*`）。
3. 组合根通过 `include_bytes!` 嵌入这些字节，连同宿主固定的名字与身份交给注册表（08 §4）。注册表启动时把它们登记为出厂代际并写入制品库（11 §4.1）。

插件工作区的 release profile：`opt-level = "s"`、`lto = true`、`strip = true`。
