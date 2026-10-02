# 11 代际与注册表（enco-kernel `registry.rs`）

P1 的主题是可恢复替换：换掉一个插件的代码，出了问题能回到上一个能用的版本。本章定义插件的身份、制品、代际、注册表、导出表、准入与健康门控。它们围绕一个概念展开，就是**代际**；其余都是代际的属性或引用。架构文档 §3.6 的归属表里，这一行是"能力注册表（代际、健康、接线）| 部署提交者（唯一）| 提交"。

P1 的插件只有 Provider。工具插件、手册与构建命令在 P2，渠道插件在 P3；它们接入时沿用本章的机制，不另立规则。

## 1. 三样东西

| 名称 | 是什么 | 存在哪里 |
|---|---|---|
| 制品 | 不可变的组件文件，按内容哈希寻址 | `$ENCO_HOME/.data/artifacts/<哈希>.wasm`，经 Store 的 `put_artifact` / `artifact` 读写（03 §3.4） |
| 代际 | 一次激活：`(编号, 插件身份, 制品哈希, 插件配置, 来源, 状态)`。编号是注册表全局自增的整数，所以一个编号就能定位一个代际，不需要插件名 | `enco.db` 的 `generations` 表（03 §3.2） |
| 调用 | 一次导出函数调用，独占一个 Store，到返回为止 | 07 §3.3，P0 已经如此 |

插件配置是 `describe(config)` 的输入。P1 的 Provider 插件没有配置，一律为空对象 `{}`。"同一份制品配上不同配置是不同代际"这条规则（架构文档 §4.5）到渠道插件出现时才有实例，P1 不加配置节。

调用参数（base_url、model、options、密钥所在的环境变量）由导入方的 profile 给出（12），属于接线，不属于代际。所以一个 Provider 代际可以同时被多个 endpoint 以不同的模型使用。

## 2. 身份：`plugins.lock`

```toml
# Maintained by enco; do not edit by hand.
[plugins.openai-compatible]
id = "01J9Z3K8Q2M4N6P8R0T2V4X6Z8"
```

- 文件在 `$ENCO_HOME/plugins.lock`，TOML。键是插件名（kebab-case），值是身份，一个 ULID。
- 只有注册表的提交者写它，经 Store 的 `register_plugin`（03 §3.5）。注册表不碰文件：名字与身份是持久化的事实，读写归 Store。CLI 不写。
- 出厂插件的名字由宿主保留，身份是宿主写死的常量（08 §4）。首次登记时写进文件。主人把出厂源码复制到 `~/.enco/plugins/openai-compatible/` 下修改、构建、部署，用的还是这个身份，所以恢复层级"出厂 ← 健康 ← 试用"不会因为修改而断开。
- 文件中出厂名字对应的身份必须等于宿主常量，否则拒绝启动，说明文件被改过。
- 没有登记的名字在第一次部署时登记：分配新的 ULID，写文件，然后才写数据库。先文件后数据库：中间崩溃只留下一条没有代际的登记，无害。
- 有登记、没有代际的插件在 `plugin status` 中列出，没有活跃代际。

P1 不检查 `~/.enco/plugins/<名字>/` 目录是否存在，也不做 `scaffold`、`rename`、`remove`（P2）。

## 3. 注册表的数据

两张表，定义在 03 §3.2：

- `generations`：编号、插件身份、制品哈希、插件配置、来源（`factory` / `deployed`）、状态（`trial` / `healthy` / `failed`）、创建时间。只追加，状态会变。
- `plugins`：插件身份、活跃代际的编号（可以为空）。

规则：

- 每个插件任一时刻至多一个活跃代际。
- 活跃代际的状态是 `trial` 或 `healthy`。`failed` 的代际永远不再活跃；要再用这份制品，就重新部署，得到新的编号。
- **回退目标**：同一插件中，编号小于当前活跃代际、状态为 `healthy` 的最大编号；没有就没有目标；插件可能没有出厂代际，也可能已经处于最早的健康代际。
- **出厂代际**：来源为 `factory`，制品是当前二进制里嵌入的那一份。宿主升级后制品哈希变了，就是一个新的出厂代际；旧的出厂代际仍然是普通的健康代际。

## 4. 归属者：`Registry`

```rust
pub struct Registry { /* 见下 */ }

pub struct RegistryDeps {
    pub store: Arc<dyn Store>,
    pub runtime: Arc<dyn Runtime>,          // 04 §2
    pub factory: Vec<FactoryPlugin>,
    pub wiring: Vec<Use>,                   // §6
    pub clock: Arc<dyn Clock>,
}

/// 嵌入宿主二进制的出厂插件；`id` 是宿主写死的身份。
pub struct FactoryPlugin { pub name: String, pub id: PluginId, pub artifact: Vec<u8> }

impl Registry {
    pub async fn open(deps: RegistryDeps) -> Result<Arc<Registry>, RegistryError>;
    /// 当前发布的导出表。每次读取得到一份不可变的快照。
    pub fn exports(&self) -> Arc<Exports>;
    pub async fn deploy(&self, name: &str, artifact: Vec<u8>) -> Result<Deployed, RegistryError>;
    pub async fn rollback(&self, name: &str) -> Result<GenerationRecord, RegistryError>;
    /// 调用方结算之后回报一次（§7）。返回值非空说明这次回报触发了回退。
    pub async fn report(&self, generation: GenerationId, verdict: Verdict) -> Result<Option<RolledBack>, RegistryError>;
    pub async fn status(&self) -> Vec<PluginStatus>;
}

pub struct Deployed { pub generation: GenerationRecord, pub users: Vec<String> }
pub enum Verdict { Ok, Failed(Failure) }
pub struct RolledBack { pub plugin: String, pub from: GenerationId, pub to: Option<GenerationId>, pub failure: Failure }

pub struct PluginStatus {
    pub name: String,
    pub id: PluginId,
    pub active: Option<GenerationRecord>,
    pub summary: Option<String>,            // 活跃制品的描述，不作为身份
    pub rollback_target: Option<GenerationId>,
    pub users: Vec<String>,
    pub generations: Vec<GenerationRecord>,   // 按编号升序
}
```

内部状态：名字到身份的映射（经 Store 读自 `plugins.lock`）、全部代际记录、每个插件的活跃编号、已加载的代际（`Loaded`，04 §2）、试用计数（§7），以及一份 `ArcSwap<Exports>`。前面几项放在一个 `tokio::sync::Mutex` 里，这把锁就是唯一的提交者：读状态、检查前置条件、写数据库、重建并发布 `Exports`，都在持锁期间按顺序完成。加载制品不在锁内（§5）。

已加载的代际只保留两类：每个插件的活跃代际，以及当前二进制的出厂代际。提交后不再属于这两类的加载结果随之丢弃，在途的调用靠自己持有的 `Arc` 完成（架构文档 §4.5）。回退目标在需要时才加载。

### 4.1 启动

`Registry::open` 按顺序做六件事，任何一步失败都拒绝启动，错误带上插件名：

1. `store.plugin_names()` 读名字与身份。核对出厂名字的身份（§2）。
2. 读注册表；代际引用的插件身份未在 `plugins.lock` 登记时，返回 `UnknownPlugin`。
3. 登记出厂制品：把每份出厂制品写入制品库；没有 `(该插件, 该哈希, factory)` 的健康代际时，插入一条 `healthy` 的出厂代际；活跃代际为空、或者活跃代际的来源也是 `factory` 时，把这条设为活跃。主人自己部署过的代际不动。
4. 加载：每个插件的活跃代际，以及第 3 步登记的当前出厂代际（安全模式用，§7）。两者相同时只加载一次。出厂代际加载失败则拒绝启动，这说明宿主自身坏了；活跃代际加载失败时，走 §5 的激活路径回退到回退目标，把它标为 `failed`，原因写日志，继续启动。
5. 接线检查（§6）。
6. 发布第一份 `Exports`。

每一步都是幂等的：中途崩溃后再启动，得到同样的结果。

### 4.2 导出表：`Exports`

```rust
pub struct Exports { /* 名字 → 活跃代际与出厂代际的导出；编号 → 身份 */ }

pub struct Export<T: ?Sized> { pub plugin: PluginId, pub generation: GenerationId, pub adapter: Arc<T> }

impl Exports {
    /// `safe_mode` 为真时，有出厂代际的插件返回当前二进制的出厂代际，其余返回活跃代际。
    pub fn completion(&self, plugin: &str, safe_mode: bool) -> Result<Export<dyn Provider>, Failure>;
    pub fn embedding(&self, plugin: &str) -> Result<Export<dyn Embedding>, Failure>;
    /// 任何已登记的代际属于哪个插件；扩展字段的回放用它（04 §6.5）。
    pub fn plugin_of(&self, generation: GenerationId) -> Option<PluginId>;
}
```

- 找不到插件、没有活跃代际、活跃代际不导出所需接口，三种情况都返回 `Failure { code: "plugin.unavailable", retryable: false }`，message 说明是哪一种。
- 每次提交后在锁内整体重建一份新的 `Exports` 并用 `ArcSwap` 发布。读者 `load_full()` 拿到的是不可变的一份，用完即弃。P1 的读者是 Attempt（04 §6.4）、记忆的 `sync`（06 §3.3）和 `plan::resolve`（04 §6.5）；它们都按调用解析，不按 Round 钉住。Round 钉住导出表是 P2 工具插件的事。

## 5. 部署与回退

活跃代际只有两种改法：部署一个新代际，或者激活一个已有的代际。加载制品要几十毫秒，所以都在锁外加载、锁内提交。

`deploy(name, artifact)`：

1. 制品写入制品库。写入按内容寻址、幂等，不需要锁。
2. `runtime.load(制品, {})`。失败返回 `Rejected`：制品不是组件、导入不满足（WIT 不匹配）、`describe` 失败，message 原样带出。
3. `probe()`（§7）。失败返回 `Rejected`，不写任何记录。
4. 取锁：没有登记的名字先登记（§2）；准入检查（§6）；插入代际并设为活跃，状态为 `trial`（M11 先用 `healthy`，试用到 M14 才有，09 §4）；重建并发布 `Exports`；释放锁。

两个部署并发时，各自在锁外加载，提交按顺序进行，最终的导出表包含两次更新。部署的意思是"换掉现在活跃的那个"，所以不核对期间活跃代际有没有变。同一份制品再次部署得到新的编号，不去重；只有出厂代际在启动时去重。

**激活**是手动回退、自动回退（§7）和启动恢复（§4.1）共用的一条路径：

```text
调用方持锁：同时确定 expected（当前活跃代际）和目标
activate(插件, expected, 目标, 待标为失败的代际列表):
    1. 锁外准备：目标未加载时 runtime.load(制品, 配置)
       加载失败只收集失败编号，继续选择更早的健康候选；候选耗尽时结束
    2. 持锁：活跃代际不再是 expected，或目标已被标为 failed 时返回 Conflict，不提交本次准备的写入
    3. 同一事务提交失败编号列表与活跃代际，更新内存，再发布 Exports
```

第 2 步的核对就是架构文档 §3.6 说的"携带期望的当前代际"：准备期间有人部署了新代际，这次激活就作废。手动回退遇到 `Conflict` 把错误返回给主人，重试即可；自动回退遇到它直接忽略，因为失败的那个代际已经不是活跃代际，与迟到的回报是同一条规则。

`rollback(name)`：持锁同时取当前活跃编号与 §3 的回退目标，没有则返回 `NoRollbackTarget`；然后 `activate(插件, expected, 目标, [])`。当前活跃代际的状态不变，它没有坏，只是主人不想用。候选全部无法加载时，提交候选的失败事实，保留当前活跃代际并返回 `NoRollbackTarget`；启动恢复与自动回退则允许活跃代际为空。

提交的顺序是：制品完整写入 → 检查 → SQLite 事务 → 发布内存中的导出表。数据库已提交、导出表尚未发布时崩溃，下次启动按数据库重建，结果相同。

只有制品缺失、损坏或 Runtime 加载失败能判定候选不可用；底层存储故障直接传播，不据此把代际标为失败。

## 6. 准入：接线

接线是"谁在用哪个插件的哪个接口"。P1 的使用者只有两种，都来自配置（12）：

| 使用者 | 插件 | 接口 |
|---|---|---|
| `profile <名字>.reply`、`profile <名字>.compaction` | endpoint 的 `plugin` | `completion` |
| `embedding` | `[embedding].plugin` | `embedding` |

```rust
pub struct Use { pub user: String, pub plugin: String, pub interface: Interface }
pub enum Interface { Completion, Embedding }
```

检查只有一个函数：给定插件和一份 `Loaded`，它的每个使用者需要的接口都在导出中。三处调用：

- 启动时对每个插件的活跃代际检查。引用了未登记的插件、插件没有活跃代际、缺接口，都拒绝启动，错误分别说明原因并列出使用者。
- 部署时对新加载的制品检查。失败返回 `Rejected`，message 形如 `used by profile default.reply, embedding; the artifact does not export completion`。部署是有意的命令，不能拆掉在用的接线。
- 回退时不检查。回退是机械恢复，可以拆掉接线；之后的调用在 `Exports` 处得到 `plugin.unavailable`，Round 明确失败。

`completion` 只由内核导入（架构文档 §4.10）。P1 没有插件之间的导入，这条不需要检查。

## 7. 健康门控

健康只统计能归因于代际本身的失败：

| 失败码 | 含义 | 谁产生 |
|---|---|---|
| `plugin.trap` | wasmtime 在实例化或调用中报错：trap、资源超限、导出缺失 | enco-wasm（07 §3.3） |
| `plugin.contract` | 插件的返回违反契约：角色不对、JSON 字段无法解析、向量数量或维度不对 | enco-wasm 的转换 |
| probe 失败 | `lifecycle.probe` 返回错误 | 部署时（§5） |

`provider.*`、`timeout`、`cancelled` 是外部失败或主人的操作，在任何代际上都可能出现，不计入。

- **回报**：每个调用插件导出的地方，在结算之后回报一次 `report(代际编号, verdict)`。P1 有两处：内核的 Attempt（04 §6.4）和记忆的 `sync`（06 §3.3）。回报带着代际编号，所以迟到的回报不会动到更新之后的代际。
- **处理**（锁内）：
  - 代际不是其插件的活跃代际：忽略。
  - 状态 `healthy`：忽略。P1 不降级健康代际，失败已经记在 Log 里，由主人判断。
  - 状态 `trial`，`Ok`：成功计数加一；达到 `TRIAL_CALLS` 时提升为 `healthy`。
  - 状态 `trial`，`Failed` 且码可归因：`activate(插件, expected: 这个代际, 回退目标, [这个代际])`（§5），成功则返回 `RolledBack`。没有目标时活跃改为空。一次失败就回退，不设阈值。
  - 状态 `trial`，`Failed` 且码不可归因：忽略。
- 计数在内存里，重启后从零开始数。
- **回报方拿到 `RolledBack` 之后**：Attempt 按 04 §6.4 用新的代际再试；并且把它作为一个 Event 送进自己的 Inbox，来源 `EventSource::Registry`，内容 `EventBody::GenerationRolledBack`（03 §1.4），下一个 Round 模型就能看到。这就是架构文档 §4.6 说的 `deploy.rolled_back`。记忆没有 Session，只写一条 warn 日志，回退的事实在 `plugin status` 里能看到。
- **probe**：`lifecycle.probe()` 只做自检，不联网，不依赖主人的配置。P1 的 Provider 插件 probe 直接返回成功；它的意义是让"坏到实例化之后就不能用"的制品在部署时被拦下，而不是在主人下一句话时才发现。
- **安全模式**：`Exports::completion(插件, safe_mode = true)` 返回出厂代际的导出（§4.2）。没有出厂代际的插件在安全模式下仍用活跃代际，没有更好的选择。

迟到的健康结果、两个并发的部署、回退之后旧代际的失败，都由"回报带编号、提交者唯一"消解，不需要别的规则。

## 8. 命令与工具

CLI 与 Agent 工具同源，都调用 `Registry` 的方法。`Registry::deploy` 只收字节，读文件的是调用它的人：CLI 这边是守护进程（08 §6），Agent 这边是宿主的插件工具（05 §2.6），内核因此不做文件 IO。

| CLI | 工具 | 作用 |
|---|---|---|
| `enco plugin status` | `plugin_status` | `[PluginStatus]` |
| `enco plugin deploy <name> <path>` | `plugin_deploy` | 把一个组件文件部署为 `<name>` 的新代际 |
| `enco plugin rollback <name>` | `plugin_rollback` | 回到上一个健康代际 |

工具描述写清：部署后的代际处于试用，前几次调用失败会自动回退；`plugin_status` 能看到每个代际的状态与回退目标。上限 `TRIAL_CALLS` 由常量生成。

P1 的构建由主人自己完成：在 `~/.enco/plugins/<name>/` 里 `cargo build --release --target wasm32-wasip2`，然后把产物路径交给 `deploy`。出厂插件依赖 `provider-protocol`，复制出来的目录用 path 或 git 依赖指向 Enco 仓库即可。`plugin_build`、lint 表与结构化诊断在 P2。

## 9. 错误与常量

```rust
#[derive(thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Store(#[from] StoreError),                  // 含 plugins.lock 与制品文件的读写失败
    #[error("plugins.lock records a different identity for factory plugin {0}")]
    FactoryIdentity(String),
    #[error("unknown plugin {0}")]
    UnknownPlugin(String),
    #[error("plugin {name} rejected: {reason}")]
    Rejected { name: String, reason: String },  // 加载、probe 或准入失败
    #[error("plugin {0} has no generation to roll back to")]
    NoRollbackTarget(String),
    #[error("plugin {0}: the registry changed while preparing; retry")]
    Conflict(String),
}
```

```rust
// enco-kernel limits.rs
pub const TRIAL_CALLS: u32 = 5;
```

## 10. P1 不做

构建命令与 lint；运行时手册与 README 生成区；制品的垃圾回收；插件配置节；`scaffold` / `rename` / `remove`；工具与状态接口；健康代际的自动降级；Round 钉住导出表与 `generation_revoked`（P2 的工具插件需要）；流式补全；跨节点。
