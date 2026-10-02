# 12 Profile 与接线（enco-kernel `profile.rs`）

Session 用哪个模型、什么参数，由它的 profile 决定。Session 只记 profile 的名字；profile 写在 `config.toml` 里，按 Attempt 的用途各选一个 endpoint。这是架构文档 §4.10 接线表中"`completion` | 每种 Attempt 用途一个 | Session 配置"这一行。

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
// enco-kernel profile.rs
/// 一个模型端点：哪个插件、什么参数、什么预算。密钥只在内存里。
pub struct Endpoint {
    pub plugin: String,
    pub settings: ProviderSettings,
    pub api_key: Option<String>,
    pub budget: Budget,              // 04 §2，context_tokens 与 max_output_tokens
}

/// 一份命名的 Session 配置。
pub struct Profile {
    pub reply: Endpoint,
    pub compaction: Endpoint,
    pub requires_lifeline: bool,
}

impl Profile {
    pub fn endpoint(&self, purpose: AttemptPurpose) -> &Endpoint;
}
```

`SessionRecord.profile` 是 profile 的名字（03 §1.9）。Session 没有单独的配置记录：`requires_lifeline` 在 profile 里。

## 2. 配置

08 §3 给出完整的 `config.toml`。与本章有关的部分：

```toml
[endpoint.chat]
plugin = "deepseek"
base_url = "https://api.deepseek.com"
model = "deepseek-chat"
api_key_env = "DEEPSEEK_API_KEY"
window_tokens = 128000
max_output_tokens = 8192
options = {}

[endpoint.cheap]
plugin = "openai-compatible"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
window_tokens = 128000
max_output_tokens = 4096

[profile.default]
reply = "chat"
compaction = "cheap"
requires_lifeline = true
```

- `[profile.default]` 必须存在，新 Session 用它。
- endpoint 可以被多个 profile、多个用途共用；同一个插件可以出现在多个 endpoint 里，各自的模型与参数不同。
- 读取配置时的检查：每个 profile 引用的 endpoint 存在；每个 endpoint 的 `window_tokens` 与 `max_output_tokens` 为正且后者小于前者；给出了 `api_key_env` 的变量都已设置，密钥读进 `Endpoint.api_key`。插件名是否存在、是否导出 `completion`，由注册表在启动时检查（11 §6）。

## 3. 什么时候解析

- 配置在启动时读取一次，交给内核：`KernelDeps.profiles: BTreeMap<String, Profile>`。修改 `config.toml` 要重启守护进程。
- **Round 开始时**按 `session.profile` 取 Profile。没有这个名字时，这个 Round 以 `Failed { code: "profile.unknown" }` 结束，Run 失败。主人下一条消息到来时，模型从 `previous_run_end` 看到这件事；主人改配置后重启，或者用 `enco profile` 换一个名字（§4）。
- **Attempt 开始时**取 `profile.endpoint(用途)`，再从导出表取 `exports.completion(endpoint.plugin, safe_mode)`（11 §4.2）。`AttemptStarted` 记录 `provider: CodeRef::Generation { id: export.generation }` 与 `settings: endpoint.settings`；调用 `export.adapter.complete(&endpoint.settings, endpoint.api_key.as_deref(), request)`（04 §6.4）。
- 密钥的路径只有一条：配置读取时从环境变量取出，放在 `Endpoint.api_key` 里，调用时传给适配器。它不进入 Log、计划、`status` 输出和日志。

两个 Session 用不同的 profile，各自的 Attempt 记录写明各自的模型与参数；同一个 Session 的回复与压缩用不同的 endpoint，两类 Attempt 记录也不同。事后看 Log 就能知道每次请求用了什么，不需要去翻当时的配置。

## 4. Session 的 profile

- `Kernel::open_session(name)` 创建 Session 时 profile 为 `default`。渠道的 `/session <名字>` 也经它创建（10 §3）。
- `Kernel::set_profile(session, profile)`：名字必须在 `profiles` 中，否则返回 `KernelError::UnknownProfile`；写入 `sessions.profile`。下一个 Round 开始时生效，正在进行的 Round 不受影响。
- CLI：`enco profile <session> <profile>`（08 §1）。

Session 存名字而不是复制一份配置，是为了让主人改一次 `config.toml` 就对所有用这个 profile 的 Session 生效。审计不受影响：每次 Attempt 记录的是实际用到的参数。

## 5. 预算与 composer

`ComposeInput` 带整份 `Profile`（04 §2），预算与 `requires_lifeline` 都从它读：

- 回复计划用 `profile.reply.budget`：常驻指令与记忆的比例、是否压缩的阈值、`max_output_tokens`，都按它算（05 §4.2–§4.4）。
- 压缩计划用 `profile.compaction.budget`：`max_output_tokens = min(COMPACTION_OUTPUT_TOKENS, compaction.budget.max_output_tokens)`；压缩请求的估算总量必须放进 `compaction.budget.context_tokens`，放不进就返回 `ContextOverflow`，窗口写压缩模型的窗口（05 §4.5）。
- `plan::validate` 的救生集检查读 `profile.requires_lifeline`（04 §6.5）。
