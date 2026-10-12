# 12 内核的配置：endpoint 与 profile（enco-kernel `settings.rs`、`profile.rs`）

Session 用哪个模型、什么参数，由它的 profile 决定。Session 只记 profile 的名字；profile 写在 `config.toml` 里，为每种 Attempt 用途按名字选一个 endpoint。这就是架构文档 §4.10 所说的"每种 Attempt 用途接一个 `completion`"。

内核是 `[owner]`、`[run]`、`[endpoint.*]`、`[profile.*]` 这几张表的归属者（架构文档 §4.11），注册表求出声明时调用它的解析（11 §5）。

## 1. 类型

```rust
// enco-core provider.rs
/// Provider 的调用参数，记入每次 AttemptStarted（03 §1.5）。不含密钥，只含密钥所在的环境变量名。
pub struct ProviderSettings {
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub options: serde_json::Value,
}
```

```rust
// enco-kernel settings.rs
/// 内核几张表解析后的值，由注册表求出声明时解析（11 §5）。
pub struct KernelSettings {
    pub timezone: Tz,                                // [owner]，主人的时区（架构文档 §3.7）
    pub max_rounds_per_run: NonZeroU32,                     // [run]
    endpoints: BTreeMap<String, EndpointSpec>,   // [endpoint.*]
    pub profiles: BTreeMap<String, Profile>,         // [profile.*]，必须含 "default"
}

impl KernelSettings {
    /// 内核拥有的表，注册表据此判断每张表的归属（11 §5）。
    pub const TABLES: [&'static str; 4] = ["owner", "run", "endpoint", "profile"];
    /// 解析并校验内核的几张表。文档中不属于内核的表原样忽略，它们由各自的归属者处理。
    pub fn from_document(document: &serde_json::Value) -> Result<KernelSettings, SettingsError>;
    /// 每个 profile 两个用途的接线（11 §6）：回复声明 `Need::Completion { tools: true }`，
    /// 压缩声明 `Need::Completion { tools: false }`。
    pub fn uses(&self) -> Vec<Use>;
    /// 查找公共 endpoint，错误带调用者所属的表。
    pub fn endpoint(&self, name: &str, table: &str) -> Result<&EndpointSpec, SettingsError>;
}

/// 一个模型服务：哪个插件、什么参数，以及模型自身的事实。不含密钥的值。
/// 事实是否齐备由使用它的一方检查：profile 要窗口与输出上限，记忆的 embedding 要维度（06 §7）。
pub struct EndpointSpec {
    pub endpoint: Endpoint,
    pub window_tokens: Option<NonZeroU32>,
    pub max_output_tokens: Option<NonZeroU32>,
    pub dimensions: Option<NonZeroUsize>,
}

/// 所有使用者共用的已解析 endpoint。
pub struct Endpoint {
    pub name: String,
    pub plugin: String,
    pub settings: ProviderSettings,
}

/// profile 所需的补全预算由 profile 解析时验证。
pub struct CompletionEndpoint {
    pub endpoint: Endpoint,
    pub budget: Budget,              // 04 §2，两个字段都是 NonZeroU32
}

/// 一份命名的 Session 配置。
pub struct Profile {
    pub reply: CompletionEndpoint,
    pub compaction: CompletionEndpoint,
    pub requires_lifeline: bool,
}

impl Profile {
    pub fn endpoint(&self, purpose: AttemptPurpose) -> &CompletionEndpoint;
}

#[derive(thiserror::Error)]
#[error("[{table}]: {message}")]
pub struct SettingsError { pub table: String, pub message: String }   // 表名形如 "owner"、"endpoint.chat"、"profile.default"、"memory"
```

`SessionRecord.profile` 是 profile 的名字（03 §1.9）。Session 没有单独的配置记录：`requires_lifeline` 在 profile 里。

## 2. 配置

内核几张表的写法见 08 §3 的示例。

检查（`from_document`），任何一条不通过都返回 `SettingsError`，指明是哪一张表：

- `[owner] timezone` 必填，是有效的 IANA 名称。不退回到节点的系统时区（架构文档 §3.7）。
- `[run] max_rounds` 可选，默认 24，必须为正。
- 每个 endpoint 的 `plugin`、`base_url`、`model` 非空；给出的窗口、输出上限、维度都为正。`api_key_env` 只记变量名，这里不检查变量是否存在。
- `[profile.default]` 必须存在，新 Session 用它。每个 profile 的 `reply` 与 `compaction` 都引用存在的 endpoint，而且这个 endpoint 写了 `window_tokens` 与 `max_output_tokens`，后者小于前者。
- endpoint 可以被多个 profile、多个用途共用；同一个插件可以出现在多个 endpoint 里，各自的模型与参数不同。

插件名是否存在、是否导出 `completion`、凭证声明与 endpoint 是否匹配，由注册表按接线检查（11 §6）。

## 3. 什么时候生效

- **应用配置时**登记，见 11 §5。
- **Round 开始时**从注册表的视图按 `session.profile` 取 Profile（04 §6.2）。没有这个名字时，这个 Round 以 `Failed { code: "profile.unknown" }` 结束，Run 失败；只有机械回退会留下这样的 Session（11 §5）。主人下一条消息到来时，composer 根据 `Transcript.run_ends` 在输入说明中呈现这件事（05 §4.3）。
- **Attempt 开始时**取 `profile.endpoint(用途)`，再从最新的视图取 `exports.completion(endpoint.plugin, safe_mode)`（11 §4.2）。`AttemptStarted` 记录 `provider: export.generation` 与 `settings: endpoint.settings`；调用 `export.adapter.complete(&endpoint.settings, request)`（04 §6.4）。
- `settings` 里只有密钥所在的变量名，值只出现在传输层（架构文档 §4.11、07 §3.3）。

## 4. Session 的 profile

- `Kernel::open_session(name)` 创建 Session 时 profile 为 `default`。渠道的 `/session <名字>` 也经它创建（10 §3）。
- `Kernel::set_profile(session, profile)`：确认 Session 存在之后经 `Registry::set_profile` 写入（11 §5）；名字不在活跃配置代际中时返回 `KernelError::UnknownProfile`。下一个 Round 开始时生效，正在进行的 Round 不受影响。
- CLI：`enco profile <session> <profile>`（08 §1）。
- 删掉或改名一个仍有 Session 在用的 profile，应用配置会被拒绝（11 §5）；先用 `enco profile` 把它们切走，再应用。

Session 存名字而不是复制一份配置，是为了让主人改一次 `config.toml` 并应用之后，就对所有用这个 profile 的 Session 生效。审计不受影响：每次 Attempt 记录的是实际用到的参数。

## 5. 预算与 composer

`ComposeInput` 带整份 `Profile`（04 §2），预算与 `requires_lifeline` 都从它读：

- 回复计划用 `profile.reply.budget`：常驻指令与记忆的比例、是否压缩的阈值、`max_output_tokens`，都按它算（05 §4.2–§4.4）。
- 压缩计划用 `profile.compaction.budget`，规则见 05 §4.5。
- `plan::validate` 的救生集检查读 `profile.requires_lifeline`（04 §6.5）。
