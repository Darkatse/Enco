# 07 插件边界：WIT、enco-wasm 与 Provider 插件

P0 提供 OpenAI-Compatible 与 DeepSeek 两个 Provider 插件，共享 `plugins/provider-protocol` 中的 WIT 绑定和线上协议转换代码。它们都以 Wasm 组件运行，作为出厂代际嵌入宿主二进制。它的意义在于让插件边界从第一天起真实存在：插件只看到 WIT，宿主只看到端口。

## 1. WIT（`wit/`）

包 `enco:plugin@0.1.0`。P0 只包含下面这些条目；架构文档 §4.4 中的其他接口（tools、channel、observer、状态与 blob 导入）在后续阶段加入，**P0 不要预先定义**。

消息与工具调用是协议本身，用 WIT 类型表达；只有工具的参数与 schema 这类业务内容是 JSON 字符串。

契约只有一份来源：[`wit/plugin.wit`](../../wit/plugin.wit)，它的文档注释生成 [`wit/CONTRACT.md`](../../wit/CONTRACT.md)（§2）。规格不另存 WIT 副本。P0 的契约由以下部分组成：

| 条目 | 内容 |
|---|---|
| `types` | `json`、`failure`、`role`、`tool-call`、`tool-result`、`extension { provider, data }`、`part`、`message`、`tool-spec` |
| `host` | 宿主导入：`log`，以及异步的 `http`（失败带 `request-sent`，§3.2） |
| `provider` | `settings`、`request`、`usage`、`stop-reason`、`completion`，以及异步的 `complete` 与 `embed` |
| `lifecycle` | `describe`，返回插件名与版本 |
| `base` / `provider-plugin` | `base` 导入 `host`、导出 `lifecycle`；`provider-plugin` 在其上导出 `provider` |

每个 world、interface、type、function 都必须有英文的 `///` 文档（02 §7），09 §2 的门禁会检查。

失败码是普通字符串，与内核使用的码（03 §1.6）完全相同，不做任何转换。

流式补全在后续阶段以新增函数的方式加入，不改变 `complete`。

## 2. CONTRACT.md

`wit/CONTRACT.md` 由 `cargo xtask docs` 生成（09 §2），不得手改。

## 3. enco-wasm

### 3.1 Engine（`engine.rs`）

```rust
pub struct WasmEngine { engine: wasmtime::Engine, ticker: JoinHandle<()> }
```

- `Config`：`wasm_component_model_async(true)`、`epoch_interruption(true)`；其余用默认值（Cranelift）。
- 启动一个 epoch 计时器任务：每 `EPOCH_TICK`（10 ms）调用一次 `engine.increment_epoch()`。这是 enco-wasm 唯一的后台任务，在 `WasmEngine` 被 drop 时 abort。
- P0 每次启动都从字节编译组件（原型实测约 27 ms），**不使用** `Component::deserialize`，因此不需要 unsafe。

### 3.2 宿主导入（`host_imports.rs`）

- `log`：转发到 `tracing`，target 为 `plugin`，附带插件名。
- `http`：使用一个共享的 `reqwest::Client`（rustls）。
  - 超时：请求中的 `timeout-ms`，否则 `HTTP_DEFAULT_TIMEOUT`（300 s）。
  - 响应体上限 `HTTP_MAX_BODY_BYTES`（32 MiB），超出返回 `too-large`。
  - 失败分类：URL 或方法无效 → `invalid-request`，`request-sent = false`；连接失败 → `connect`，`request-sent = false`；超时 → `timeout`，`request-sent = true`；读取响应体失败 → `body`，`request-sent = true`；其他 → `other`，`request-sent = true`。
- 不在任何日志中记录请求头（其中有 API key）。

### 3.3 WasmProvider（`provider.rs`）

实现内核的 `Provider` 端口。

```rust
pub struct WasmProvider {
    pre: ProviderPluginPre<HostState>,   // bindgen 生成的 InstancePre 包装，启动时构造一次
    engine: Arc<WasmEngine>,
    artifact: ContentHash,
    settings: ProviderSettings,          // base_url、model、api_key、options；来自配置，不进入 Log
    http: reqwest::Client,
}

impl WasmProvider {
    pub async fn new(engine: Arc<WasmEngine>, component_bytes: &[u8], settings: ProviderSettings) -> Result<Self, WasmError>;
}
```

```rust
/// 来自配置与环境变量（08 §3），只在调用插件时传入，不写入 Log。
pub struct ProviderSettings { pub base_url: String, pub model: String, pub api_key: Option<String>, pub options: serde_json::Value }
```

- `code()` 返回 `CodeRef::Wasm { artifact }`，`artifact = ContentHash::of(component_bytes)`。
- `new` 是异步构造：编译组件、构造 `Linker`（`wasmtime_wasi::p2::add_to_linker_async` 加上 bindgen 生成的宿主导入）、构造 `InstancePre`，并调用一次 `describe("{}")` 检查插件可以正常实例化。失败即返回错误，守护进程拒绝启动。
- `complete` 与 `embed` 走同一条调用路径，下面的规则对两者相同。
- **每次调用一个新的 Store**（架构文档 §4.5 的"调用"）：
  - `HostState` 包含一个最小的 `WasiCtx`（只继承 stderr，没有预开放目录、没有环境变量、没有参数）、`ResourceTable`、`reqwest::Client` 的克隆、插件名。
  - `StoreLimits`：线性内存上限 `WASM_MEMORY_LIMIT`（256 MiB）。
  - `store.epoch_deadline_async_yield_and_update(1)`：客户代码每个 epoch 让出一次，使外层的超时与取消能够生效。
  - 用 `tokio::time::timeout(PROVIDER_CALL_TIMEOUT, …)`（330 s）包住整个调用；超时返回 `Failure { code: "timeout", retryable: true }`。
  - 调用通过 `store.run_concurrent(async |accessor| …)` 进行（异步导出需要它）。
  - 调用中的 wasmtime 错误（trap 或实例化失败）返回 `Failure { code: "provider.bad_response", message: "plugin call failed: …", retryable: false }`。Store 随调用结束而丢弃。
- `convert.rs` 负责 enco-core 与 WIT 类型之间的双向转换：
  - 请求：`ToolCall` → `tool-call { id: provider_id, … }`；`ToolResult` → `tool-result { call-id: provider_id, … }`。
  - 结果：每个 `tool-call` 生成一个新的 `CallId`，`provider_id` 取自 `tool-call.id`。

原型中验证过的 wasmtime 49 细节：`wasmtime::Error` 是独立的类型，不是 `anyhow::Error`；bindgen 为异步导入生成 `HostWithStore<U>` trait，实现在 `HasSelf<HostState>` 上；`list` 是 WIT 关键字。

### 3.4 常量（`limits.rs`）

```rust
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
pub const WASM_MEMORY_LIMIT: usize = 256 * 1024 * 1024;
pub const PROVIDER_CALL_TIMEOUT: Duration = Duration::from_secs(330);
pub const HTTP_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
pub const HTTP_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
```

## 4. Provider 插件（`plugins/`）

两个薄 `cdylib` 入口复用 `provider-protocol` 中生成的 WIT 绑定与 Chat Completions 转换。OpenAI-Compatible 支持补全与 embedding；DeepSeek 的 `embed` 明确返回不支持，embedding 使用独立的配置。

**扩展命名空间就是插件名。** 每个插件只定义一个常量 `PROVIDER`，同时用作 `describe` 返回的名字和 `extension.provider`（例如 `openai-compatible`、`deepseek`）。共享协议库只接受这个名字，不另起别名；插件只把自己命名空间下的扩展字段合并回请求，所以一个服务的额外字段（例如推理内容）不会发给另一个服务。DeepSeek 插件目前没有专有逻辑，它存在的作用就是这个独立的命名空间。

### 4.1 请求

`POST {base-url}/chat/completions`，请求头 `Content-Type: application/json`，有 `api-key` 时加 `Authorization: Bearer {api-key}`。请求体：

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
  - assistant → `{ "role": "assistant", "content": <文本或 null>, "tool_calls": [ { "id", "type": "function", "function": { "name", "arguments" } } ] }`（没有工具调用时省略 `tool_calls`）；本插件产生的 `extension` 中的字段合并回这条消息对象。
  - tool → `{ "role": "tool", "tool_call_id": <call-id>, "content" }`

### 4.2 响应

从 `choices[0].message` 读取：

- `content`（字符串，可能为 null）→ `text` 部分。
- `tool_calls` → `tool-call` 部分，`arguments` 原样保留为字符串。
- 除 `role`、`content`、`tool_calls` 之外的字段（例如某些服务商的 `reasoning_content`）→ 一个 `extension { provider: <本插件的 PROVIDER>, data: {这些字段} }`，下次请求时合并回去（§4.1）。
- `finish_reason`：`stop` → `end-turn`，`tool_calls` → `tool-calls`，`length` → `max-tokens`，其他 → `other`。
- `usage.prompt_tokens`、`usage.completion_tokens`、`usage.prompt_tokens_details.cached_tokens`（可能不存在）→ `usage`。

### 4.3 嵌入（`embed`）

`POST {base-url}/embeddings`，请求头同 §4.1，`http-request.timeout-ms` 为 30000。请求体：

```json
{ "model": "<settings.model>", "input": ["...", "..."] }
```

- `settings.options` 按 §4.1 的规则合并（例如 `{ "dimensions": 512 }`）。
- 响应：`data` 中每一项的 `embedding`（数字数组）按 `index` 排序后返回。兼容接口可以省略值为零的 `index`，插件统一解码为 0；之后仍校验位置覆盖完整且没有重复，绝不按任意缺项猜测顺序。`data` 的数量与 `inputs` 不同，或者缺少 `embedding`，返回 `provider.bad_response`。
- 维度由宿主核对（06 §3.3），插件不关心。

### 4.4 失败分类

| 情况 | code | retryable |
|---|---|---|
| `http-failure`（任何类别） | `provider.network`；类别为 `timeout` 时为 `timeout` | true |
| 状态 401、403 | `provider.auth` | false |
| 状态 429 | `provider.rate_limited` | true |
| 状态 5xx | `provider.server` | true |
| 其他非 2xx | `provider.bad_request` | false |
| 响应不是合法 JSON，或缺少 `choices[0].message`（嵌入时见 §4.3） | `provider.bad_response` | false |

非 2xx 时，`message` 包含状态码和响应体的前 500 字节。

模型请求没有需要结算的外部效果（至多重复计费），所以网络失败一律可以重试。是否重试由调用方决定：内核只重试 `complete`（04 §6.4），记忆不重试 `embed`（06 §3.3）。

## 5. 出厂代际的嵌入

1. `cargo xtask build-factory` 构建整个插件工作区（`--workspace --target wasm32-wasip2 --release`），把 `provider_openai.wasm` 与 `provider_deepseek.wasm` 复制到 `target/factory/`，文件名分别为 `provider-openai.wasm` 与 `provider-deepseek.wasm`。
2. `apps/enco/build.rs` 检查两份制品存在；缺失则提示先运行 `cargo xtask build-factory`。为每份制品输出 `cargo:rerun-if-changed` 和绝对路径环境变量（`ENCO_FACTORY_OPENAI`、`ENCO_FACTORY_DEEPSEEK`）。
3. 组合根通过 `include_bytes!` 嵌入两份字节；`factory` 只把明确的配置名称映射到制品，不识别 URL 或处理服务商协议。
4. 启动时把所用制品写入 blob 存储，使 Log 中 `CodeRef::Wasm { artifact }` 的哈希可以找到对应字节。
5. 按 `[provider].plugin` 与 `[embedding].plugin` 分别构造 `WasmProvider`，前者交给内核，后者交给记忆。两者可以选择相同制品并使用不同设置，也可以分别选择 DeepSeek 与 OpenAI-Compatible。

插件工作区的 release profile：`opt-level = "s"`、`lto = true`、`strip = true`。
