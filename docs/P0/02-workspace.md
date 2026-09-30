# 02 工作区、crate 与编码规范

## 1. 工具链

`rust-toolchain.toml`：

```toml
[toolchain]
channel = "1.98.1"
components = ["rustfmt", "clippy"]
targets = ["wasm32-wasip2"]
```

Edition 2024，`resolver = "3"`。只使用 stable。

**关于 WASIp3。** WASIp3 的核心是组件模型的异步 ABI（`async func`、`stream`、`future`），我们的 WIT 从第一天起就使用它；在 `wasm32-wasip2` 目标上配合 wit-bindgen 0.62 与 wasmtime 49 已经实测可用。Rust 的 `wasm32-wasip3` 目标在 1.98.1 中仍是 Tier 3（rustup 没有预编译的 std，需要 nightly 与 `-Zbuild-std`），与"只使用 stable"冲突，而且插件几乎不经 std 使用系统接口，换目标没有功能收益。宿主一侧 wasmtime-wasi 49 的默认 feature 已包含 p3。该目标进入 Tier 2 之后再迁移，只涉及三处：本文件的 `targets`、`xtask build-factory` 的 `--target`（07 §5）、enco-wasm 中的 WASI linker（07 §3.3）。WIT 不依赖 WASI 版本，所以不变。

## 2. 目录结构

```text
Cargo.toml                 # 工作区：crates/*、apps/enco、xtask
rust-toolchain.toml
clippy.toml
.cargo/config.toml         # 只有一行：xtask 别名（09 §1）
crates/
  enco-core/               # 领域类型：纯数据 + serde，无 IO
  enco-kernel/             # 端口、Kernel、Session actor、Run/Round、恢复、调度；无 IO 实现
  enco-host/               # SQLite Store、blob 存储、原生工具、记忆、出厂 composer、工作区上下文源、系统时钟
  enco-wasm/               # wasmtime 嵌入、WIT 绑定、Provider 端口的 Wasm 实现、宿主导入（log、http）
apps/
  enco/                    # 组合根：CLI、守护进程、本地协议、配置、出厂代际嵌入
xtask/                     # cargo xtask：build-factory、boundaries、docs、check
plugins/                   # 独立的 Cargo 工作区，目标 wasm32-wasip2
  provider-openai/
  provider-deepseek/
  provider-protocol/      # 插件内部共享 WIT 绑定与线上协议；不依赖宿主 crate
wit/                       # enco:plugin 包；CONTRACT.md 为生成物
docs/
```

`enco-sdk`、`enco-space` 在 P0 中**不创建**（架构文档 §10 中它们分别属于 P2、P4）。

插件放在独立工作区，因为它们的目标平台不同。`xtask build-factory` 负责构建插件并把产物放到 `target/factory/`（07 §5）。

## 3. crate 职责与依赖方向

```text
enco-core  ←  enco-kernel  ←  enco-host
                          ←  enco-wasm
                                         ←  apps/enco（组合根）
```

| crate | 允许依赖的工作区 crate | 允许的外部依赖 |
|---|---|---|
| enco-core | 无 | serde、serde_json、ulid、blake3、chrono、thiserror |
| enco-kernel | enco-core | tokio（rt、sync、time、macros）、tokio-util、async-trait、serde、serde_json、ulid、thiserror、tracing |
| enco-host | enco-core、enco-kernel | rusqlite（bundled）、triviumdb、tokio（rt、fs、process、io-util、time、sync）、tokio-util、async-trait、serde、serde_json、chrono、ulid、blake3、thiserror、tracing |
| enco-wasm | enco-core、enco-kernel | wasmtime、wasmtime-wasi、reqwest、tokio、async-trait、serde_json、ulid、thiserror、tracing |
| apps/enco | 以上全部 | clap、anyhow、tokio（full）、tokio-util、serde、serde_json、toml、tracing、tracing-subscriber、dirs、ulid；dev：wiremock、tempfile |
| xtask | 无 | anyhow、cargo_metadata、wit-parser、serde_json |

enco-host 与 enco-wasm 互不依赖。`xtask boundaries` 按这张表检查（09 §1）。

## 4. 依赖版本

在根 `Cargo.toml` 的 `[workspace.dependencies]` 中统一声明，各 crate 用 `workspace = true` 引用并按需开启 feature：

```toml
[workspace.dependencies]
tokio = { version = "1.53", default-features = false }
tokio-util = "0.7.19"
async-trait = "0.1.92"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
thiserror = "2.0.21"
anyhow = "1.0.104"
ulid = { version = "3.0.0", features = ["serde"] }
blake3 = "1.8.7"
chrono = { version = "0.4.45", default-features = false, features = ["clock", "std", "serde"] }
rusqlite = { version = "0.40.2", features = ["bundled"] }
triviumdb = "=0.8.8"                     # 0.x，精确锁定；只作记忆的派生索引（06 §3）
reqwest = "0.13.5"                       # 默认使用 rustls
wasmtime = "49.0.1"                      # 默认 feature 已含 component-model-async
wasmtime-wasi = "49.0.1"
clap = { version = "4.6.7", features = ["derive"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
toml = "1.1.6"
dirs = "7.0.0"
cargo_metadata = "0.23.1"
wit-parser = "0.259.0"
wiremock = "0.6.5"
tempfile = "3.27.0"
```

插件工作区（`plugins/Cargo.toml`）只依赖 `wit-bindgen = "0.62.0"`、`serde`、`serde_json`。

新增依赖必须先提问（01 §5）。

## 5. Lint

根 `Cargo.toml`：

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
```

每个 crate 都写 `[lints] workspace = true`。`unsafe_code = "forbid"` 只约束本仓库的 crate；wasmtime、TriviumDB 等依赖内部经过审计的 unsafe 不在此列。`enco-core` 与 `enco-kernel` 的 `lib.rs` 另加 `#![warn(missing_docs)]`（`xtask check` 以 `-D warnings` 运行，所以实际是错误）。

`clippy.toml`：

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

`std::sync::Mutex` 加锁使用 `lock().unwrap_or_else(PoisonError::into_inner)`。

## 6. 各 crate 的文件布局

文件名就是它负责的事情。不要出现 `utils.rs`、`helpers.rs`、`common.rs`、`types.rs`。使用 `foo.rs` + `foo/` 的形式，不使用 `mod.rs`。

**enco-core**

```text
lib.rs         重新导出；estimate_tokens
ids.rs         SessionId、EventId、RunId、RoundId、AttemptId、CallId、ScheduleId、MemoryId、NodeId、Epoch、Seq、LogPos
hash.rs        ContentHash
message.rs     Role、Message、Part、ToolCall、ToolResult、Extension
event.rs       Event、EventSource、EventBody
entry.rs       Entry、EntryBody 及其附属枚举
tool.rs        ToolSpec、Effect、Outcome、Settlement、Failure、Arguments
capability.rs  CapabilityId、CodeRef
plan.rs        ContextPlan、PlanItem、Omission、Contribution、Candidate、CandidateKind
session.rs     SessionRecord、SessionConfig、Binding、Schedule、ScheduleState
memory.rs      Memory
```

**enco-kernel**

```text
lib.rs
ports.rs       重新导出以下六个端口
ports/store.rs      Store、StoreError、Commit、NodeRecord
ports/provider.rs   Provider、ProviderRequest、Completion
ports/composer.rs   Composer、ComposeInput、Composition、Transcript、Budget、ComposeError
ports/context.rs    ContextSource、ContextQuery、ContextError
ports/tool.rs       Tool、CallContext
ports/clock.rs      Clock
kernel.rs      Kernel、KernelDeps、KernelConfig、KernelError
snapshot.rs    Snapshot（本轮可用的能力）
session.rs     Session actor：唤醒、启动恢复、驱动 Run
run.rs         Run 与 Round 的算法
attempt.rs     Attempt（含重试）与压缩 Attempt
dispatch.rs    工具调用的校验、分派、结算与结果文本
transcript.rs  Log → Transcript 的投影
plan.rs        ContextPlan 的校验与解析
recovery.rs    启动恢复（纯函数）
scheduler.rs   Scheduler actor、Schedules 句柄与 ScheduleError
builtin.rs     内核内置工具：schedule_create / schedule_list / schedule_cancel
limits.rs      常量
```

**enco-host**

```text
lib.rs
sqlite.rs        两个 SQLite 权威共用的连接设置与列编码
store.rs         SqliteStore（实现 Store 端口），含 blob 文件
store/schema.sql 建表 SQL
store/commit.rs  Commit 的前置条件与 Inbox 消费
store/rows.rs    行到领域类型的映射
tools.rs         原生工具集合与救生集
tools/args.rs    工具参数的读取与校验
tools/files.rs   fs_read、fs_write、fs_edit、fs_list
tools/shell.rs   shell_exec
memory.rs        Memories（记忆的归属者）、MemoryError（06 §7）
memory/authority.rs  memory.db
memory/index.rs      TriviumDB 索引、reconcile、sync
memory/context.rs    MemoryContextSource
memory/tools.rs      memory_save / memory_update / memory_forget / memory_search
composer.rs      出厂 composer
prompts/         system.md、compaction.md、safe_mode.md（include_str!）
context.rs       WorkspaceContextSource
clock.rs         SystemClock
limits.rs
```

**enco-wasm**

```text
lib.rs
engine.rs        Engine 配置与 epoch 计时器
provider.rs      WasmProvider（实现 Provider 端口）、ProviderSettings、WasmError
convert.rs       enco-core 类型 ↔ WIT 绑定类型
host_imports.rs  宿主导入：log、http
limits.rs
```

**apps/enco**

```text
build.rs         嵌入出厂代际
main.rs          clap 子命令分派
paths.rs         ENCO_HOME 布局
config.rs        config.toml
compose_root.rs  构造 KernelDeps
daemon.rs        enco serve：本地协议服务端
protocol.rs      请求与推送消息类型
client.rs        本地协议客户端
chat.rs          enco chat：交互与渲染
```

## 7. 编码规范

- **错误**：错误类型用 `thiserror` 定义，放在产生它的模块中：端口错误在端口文件，`KernelError` 在 `kernel.rs`，`ScheduleError` 在 `scheduler.rs`，`MemoryError` 在 `memory.rs`，`WasmError` 在 `provider.rs`。一个变体只表达一种含义，不借用其他变体。错误消息使用小写、不以句号结尾，并带上定位信息（session id、log 位置、路径）。
- **文档语言**：源码级文档使用英文，包括 rustdoc、WIT 文档、代码与 SQL 注释、工具描述和提示词；设计文档（`docs/`、`AGENTS.md`、`CONTRIBUTING.md`）使用中文。面向模型的文本中出现的上限由 `limits.rs` 中的常量生成，不另写数字。
- **异步**：不在异步上下文中阻塞。rusqlite 的调用放进 `spawn_blocking`，文件 IO 使用 `tokio::fs`。
- **日志**：使用 `tracing`。span 为 `session{id}` → `run{id}` → `round{id}`。生命周期事件用 info，细节用 debug。不在任何级别记录 API key；完整的提示词只在 trace 级别记录。
- **文档注释**：enco-core 与 enco-kernel 的公开条目必须有文档。写含义、不变式和单位，不要复述名字（"Returns the id" 这样的注释没有价值）。
- **Serde**：枚举使用 `#[serde(rename_all = "snake_case")]`；带数据的枚举使用内部标签 `#[serde(tag = "kind")]`。配置与协议输入使用 `deny_unknown_fields`；Log 条目不使用（新增的可选字段对旧的读取者无害）。
- **时间**：存储与传输一律使用 `DateTime<Utc>`，序列化为带毫秒的 RFC 3339。
- **ID**：newtype + `#[serde(transparent)]`，`Display` 输出 ULID 字符串。
- **测试位置**：纯函数的单元测试写在模块内；内核与宿主的集成测试放在 `crates/enco-host/tests/`；端到端测试放在 `apps/enco/tests/`（09 §3）。
