# 11 代际与注册表（enco-kernel `registry.rs`）

P1 的主题是可恢复替换：换掉一个插件的代码后出了问题，能回到上一个能用的版本。本章定义插件的身份、制品、代际、注册表、导出表、准入与健康门控。这些都围绕**代际**展开，其余的要么是代际的属性，要么引用代际。代际、健康状态与接线都归注册表所有，注册表是唯一的提交者（架构文档 §3.6）。

P1 的插件只有 Provider。工具插件、手册与构建命令在 P2 加入，渠道插件在 P3 加入，都沿用本章的机制。

## 1. 三样东西

| 名称 | 是什么 | 存在哪里 |
|---|---|---|
| 制品 | 不可变的组件文件，按内容哈希寻址 | `$ENCO_HOME/.data/artifacts/<哈希>.wasm`，经 Store 的 `put_artifact` / `artifact` 读写（03 §3.4） |
| 代际 | 一份已登记的插件实现：`(编号, 插件身份, 制品哈希, 插件配置, 来源, 状态, 失败原因)`。编号是注册表全局自增的整数，所以一个编号就能定位一个代际，不需要插件名 | `enco.db` 的 `generations` 表（03 §3.2） |
| 调用 | 一次导出函数调用，独占一个 Store，到返回为止 | 07 §3.3，P0 已经如此 |

插件配置是 `describe(config)` 的输入。P1 的 Provider 插件没有配置，一律为空对象 `{}`。架构文档 §4.5 规定，同一份制品配上不同配置是不同的代际；这条规则要等渠道插件出现才有实例，所以 P1 不加配置节。

调用参数（base_url、model、options、密钥所在的环境变量）由导入方的 profile 给出（12），属于接线，不属于代际。所以一个 Provider 代际可以同时被多个 endpoint 以不同的模型使用。

## 2. 身份：`plugins.lock`

```toml
# Maintained by enco; do not edit by hand.
[plugins.openai-compatible]
id = "01J9Z3K8Q2M4N6P8R0T2V4X6Z8"
```

- 文件位于 `$ENCO_HOME/plugins.lock`，格式是 TOML：键是插件名（kebab-case），值是身份（一个 ULID）。
- 文件只由注册表写入，读写经 Store 的 `register_plugin` 完成（03 §3.5）：名字与身份是持久化的事实，文件 IO 归 Store。CLI 不写这个文件。
- 出厂插件的名字由宿主保留，身份是宿主写死的常量（08 §4），首次登记时写进文件。主人把出厂源码复制到 `~/.enco/plugins/openai-compatible/`，修改后构建、部署，用的仍是这个身份，所以"出厂 ← 健康 ← 试用"的恢复层级不会因为修改而断开。
- 文件里出厂插件的身份必须等于宿主常量。不相等说明文件被改过，拒绝启动。
- 没有登记的名字在第一次部署时登记：先分配 ULID 并写入文件，再写数据库。两步之间崩溃，只会留下一条没有代际的登记，不影响正确性。
- 已登记但还没有代际的插件，也会在 `plugin status` 中列出，显示没有活跃代际。

P1 不检查 `~/.enco/plugins/<名字>/` 目录是否存在，也不做 `scaffold`、`rename`、`remove`（P2）。

## 3. 注册表的数据

两张表，定义在 03 §3.2：

- `generations`：编号、插件身份、制品哈希、插件配置、来源（`factory` / `deployed`）、状态（`trial` / `healthy` / `failed`）、失败原因（只有 `failed` 的代际有）、创建时间。记录只追加，状态与失败原因一起更新。
- `plugins`：插件身份、活跃代际的编号（可以为空）。

规则：

- 每个插件任一时刻至多一个活跃代际。
- 活跃代际的状态是 `trial` 或 `healthy`。`failed` 的代际永远不再活跃；要再用这份制品，就重新部署，得到新的编号。
- **回退目标**：同一插件中，编号小于当前活跃代际、状态为 `healthy` 的代际里编号最大的那个。找不到就没有回退目标，例如插件没有出厂代际，或者已经处在最早的健康代际。
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
    /// 调用方结算之后回报一次（§7）。`session` 接收可能产生的回退通知；返回值非空说明这次回报触发了回退。
    pub async fn report(&self, generation: GenerationId, verdict: Verdict, session: Option<SessionId>) -> Result<Option<RolledBack>, RegistryError>;
    pub async fn status(&self) -> Vec<PluginStatus>;
}

pub struct Deployed { pub generation: GenerationRecord, pub users: Vec<String> }
pub enum Verdict { Ok, Failed(Failure) }
// 定义在 enco-core，也是 EventBody::GenerationRolledBack 的 `rollback`。
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

注册表的内部状态包括：名字到身份的映射（经 Store 读自 `plugins.lock`）、全部代际记录、活跃编号映射 `BTreeMap<PluginId, GenerationId>`（没有键即没有活跃代际）、已加载的代际（`Loaded`，04 §2）、试用计数（§7），以及一份 `ArcSwap<Exports>`。除 `ArcSwap<Exports>` 外，这些状态都放在一个 `tokio::sync::Mutex` 里。这把锁就是唯一的提交者：读状态、检查前置条件、写数据库、重建并发布 `Exports`，都在持锁时按顺序完成。加载制品不持锁（§5）。

内存里只保留两类已加载的代际：每个插件的活跃代际，以及当前二进制的出厂代际。提交之后不再属于这两类的代际随即丢弃；还在进行的调用持有自己的 `Arc`，照常完成（架构文档 §4.5）。回退目标到需要时才加载。

### 4.1 启动

`Registry::open` 按顺序做六件事，任何一步失败都拒绝启动，错误带上插件名：

1. 用 `store.plugin_names()` 读出名字与身份，核对出厂插件的身份（§2）。
2. 读注册表。如果某个代际引用的插件身份没有在 `plugins.lock` 登记，返回 `UnknownPlugin`。
3. 登记出厂制品：每份出厂制品写入制品库。还没有这个插件、这个哈希的出厂健康代际时，插入一条状态为 `healthy` 的出厂代际。插件没有活跃代际，或者活跃代际本身也是出厂代际时，把当前二进制的这条出厂代际设为活跃；主人自己部署的活跃代际保持不变。
4. 加载每个插件的活跃代际，以及第 3 步登记的当前出厂代际（安全模式要用，§7）；两者是同一个时只加载一次。出厂代际加载失败说明宿主自身坏了，拒绝启动。活跃代际加载失败时，按 §5 的激活流程回退到回退目标，把失败的代际标为 `failed` 并保存加载失败的原因，然后继续启动。
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
    pub fn decision(&self, plugin: &str) -> Result<Export<dyn Decision>, Failure>;
    /// 任何已登记的代际属于哪个插件；扩展字段的回放用它（04 §6.5）。
    pub fn plugin_of(&self, generation: GenerationId) -> Option<PluginId>;
}
```

- 找不到插件、没有活跃代际、活跃代际不导出所需接口，三种情况都返回 `Failure { code: "plugin.unavailable", retryable: false }`，message 说明是哪一种。
- 每次提交后，在锁内重建一份新的 `Exports`，用 `ArcSwap` 发布。读者用 `load_full()` 取得一份不可变的快照，用完即弃。读者是 Attempt（04 §6.4）、记忆的插件调用（06 §3.3）和 `plan::resolve`（04 §6.5）。它们每次调用都重新解析，不在 Round 内钉住导出表；钉住要等 P2 有了工具插件才需要。

## 5. 部署与回退

活跃代际只有两种改法：部署一个新代际，或者激活一个已有的代际。加载制品要几十毫秒，所以两者都在锁外加载，在锁内提交。

`deploy(name, artifact)` 分四步：

1. 把制品写入制品库。写入按内容寻址，是幂等的，不需要锁。
2. 用 `runtime.load(制品, {})` 加载。制品不是组件、导入不满足（WIT 不匹配）或 `describe` 失败时，返回 `Rejected`，原样带出错误信息。
3. 调用 `probe()`（§7）。失败时返回 `Rejected`，不写任何记录。
4. 持锁提交：名字还没有登记就先登记（§2），做准入检查（§6），插入状态为 `trial` 的新代际并设为活跃，然后重建并发布 `Exports`。

两个部署并发时，各自在锁外加载，再按顺序提交，最终的导出表包含两次更新。部署的含义是"换掉现在活跃的那个"，所以不核对加载期间活跃代际是否变过。同一份制品再次部署会得到新的编号，不做去重；只有出厂代际在启动时去重。

**激活**是手动回退、自动回退（§7）和启动恢复（§4.1）共用的流程，形式为 `activate(插件, expected, 候选, 原因)`。原因是手动回退、启动恢复、试用失败三者之一，试用失败还带着故障和来源 Session。调用方先持锁确定当前活跃代际（即 expected）和候选，然后：

1. 锁外准备：按顺序取候选作为目标，还没加载就加载它。加载失败时记下这个代际及其原因，改用下一个候选，直到候选用尽。
2. 持锁核对：活跃代际已经不是 expected、目标已被标为 `failed`，或者试用失败时 expected 已经晋升为 `healthy`，都返回 `Conflict`，本次准备的写入一律不提交。
3. 在同一个事务里：把要标为失败的代际（包括第 1 步加载失败的）连同原因标为 `failed`，更新活跃代际；试用失败有来源 Session 时，把回退事件写入它的 Inbox。事务成功后更新内存，发布 `Exports`。

第 2 步就是架构文档 §3.6 所说的"携带期望的当前代际与状态"：准备期间有人部署了新代际，或者试用代际已经晋升，这次激活就作废。手动回退遇到 `Conflict` 时把错误返回给主人，重试即可。自动回退遇到时直接忽略，因为原来的试用条件已经改变，这和忽略迟到的回报是同一条规则。

`rollback(name)` 持锁取出当前活跃代际和 §3 的回退目标，没有目标就返回 `NoRollbackTarget`；有目标就激活它，调用方自己不要求标记任何代际。当前代际的状态保持不变：它没有坏，只是主人不想用。如果所有候选都加载失败，就提交这些失败，保留当前活跃代际，返回 `NoRollbackTarget`。启动恢复和自动回退则不同，候选用尽时允许插件没有活跃代际。

提交的顺序是：制品完整写入 → 检查 → SQLite 事务 → 发布内存中的导出表。数据库已提交、导出表尚未发布时崩溃，下次启动会按数据库重建，结果相同。

只有制品缺失、损坏或 Runtime 加载失败，才能判定代际不可用；统一记录为 `plugin.load`，message 保留原始错误。底层存储故障直接向上传播，不会因此把代际标为失败。

## 6. 准入：接线

接线是"谁在用哪个插件的哪个接口"。使用者有三种，都来自配置（08 §3、12）：

| 使用者 | 插件 | 接口 |
|---|---|---|
| `profile <名字>.reply`、`profile <名字>.compaction` | endpoint 的 `plugin` | `completion` |
| `embedding` | `[embedding].plugin` | `embedding` |
| `decision` | `[decision].plugin`（配置了时） | `decision` |

```rust
pub struct Use { pub user: String, pub plugin: String, pub interface: Interface }
pub enum Interface { Completion, Embedding, Decision }
```

检查只有一个函数：给定插件和一份 `Loaded`，确认这个插件的每个使用者需要的接口都在导出中。它在三处调用：

- 启动时对每个插件的活跃代际检查。引用了未登记的插件、插件没有活跃代际、缺接口，都拒绝启动，错误分别说明原因并列出使用者。
- 部署时对新加载的制品检查。失败返回 `Rejected`，message 形如 `used by profile default.reply, embedding; the artifact does not export completion`。部署是有意的命令，不能拆掉在用的接线。
- 回退时不检查。回退是机械恢复，可以拆掉接线；之后的调用在 `Exports` 处得到 `plugin.unavailable`，与其他失败一样由调用方处理。

`completion` 只由内核导入（架构文档 §4.10）。P1 还没有插件之间的导入，所以每个活跃代际都准入，这一条也暂时不需要检查。

## 7. 健康门控

健康只统计能归因于代际本身的失败：

| 失败码 | 含义 | 谁产生 |
|---|---|---|
| `plugin.trap` | wasmtime 在实例化或调用中报错：trap、资源超限、导出缺失 | enco-wasm（07 §3.3） |
| `plugin.contract` | 插件的返回违反契约：角色不对、JSON 字段无法解析、向量数量或维度不对 | enco-wasm 的转换 |
| probe 失败 | `lifecycle.probe` 返回错误 | 部署时（§5） |

`provider.*`、`timeout`、`cancelled` 是外部失败或主人的操作，在任何代际上都可能出现，不计入。

- **回报**：凡是调用了插件导出的地方，都在结算之后调用一次 `report(代际编号, verdict, 来源 Session)`：内核的 Attempt（04 §6.4）与记忆的插件调用（06 §3.3）。Attempt 传入本 Session，记忆传入 `None`。回报带着代际编号，所以迟到的回报碰不到更新之后的代际。verdict 只看插件调用本身：空摘要、向量维度与索引配置不符，是调用方自己的判断，不算插件失败。
- **处理**（持锁进行）：
  - 代际已经不是所属插件的活跃代际：忽略。
  - 代际是 `healthy`：忽略。P1 不降级健康代际。
  - 试用代际调用成功：成功次数加一，达到 `TRIAL_CALLS` 时晋升为 `healthy`。
  - 试用代际出现可以归因于它自身的失败：立即回退。以这个代际为 expected 激活回退目标，并以这次的 `Failure` 为原因把它标为失败（§5），成功则返回 `RolledBack`；没有回退目标时，插件不再有活跃代际。一次失败就回退，不设阈值。
  - 试用代际出现不能归因于它的失败：忽略。
- 成功次数按代际累计，各接口、不同 Session 与 endpoint 共用计数。外部失败既不增加也不清零；计数只记在内存里，重启后从零开始。
- **回退通知**：回报带有来源 Session 时，注册表在回退的同一事务里向它的 Inbox 投递 `EventBody::GenerationRolledBack`，来源是 `EventSource::Registry`（03 §1.4），通知文字由注册表此时写入 Event 的 `text`。这就是架构文档 §4.6 所说的 `deploy.rolled_back`。模型在下一个 Round 看到它；回退后进程立即崩溃，事件也会在重启后消费。Attempt 收到 `RolledBack` 后按 04 §6.4 立即用新代际重试。通知只发给出故障的 Session；其他 Session，包括部署它的那个，从 `plugin_status` 查看失败原因。
- **probe**：`lifecycle.probe()` 只做自检，不联网，也不依赖主人的配置。P1 的 Provider 插件直接返回成功。它的作用是让实例化之后就无法使用的制品在部署时被拦下，而不是等到主人下一句话时才暴露。
- **安全模式**：`Exports::completion(插件, safe_mode = true)` 返回出厂代际的导出（§4.2）。没有出厂代际的插件在安全模式下仍用活跃代际，没有更好的选择。

迟到的健康结果、两个并发的部署、回退之后旧代际的失败，都由"回报带编号、提交时核对期望的代际与状态、提交者唯一"消解，不需要别的规则。

## 8. 命令与工具

CLI 与 Agent 工具同源，都调用 `Registry` 的方法。`Registry::deploy` 只接收字节，文件由调用方读取：CLI 这边是守护进程（08 §6），Agent 这边是宿主的插件工具（05 §2.6）。内核因此不做文件 IO。

| CLI | 工具 | 作用 |
|---|---|---|
| `enco plugin status` | `plugin_status` | `[PluginStatus]` |
| `enco plugin deploy <name> <path>` | `plugin_deploy` | 把一个组件文件部署为 `<name>` 的新代际 |
| `enco plugin rollback <name>` | `plugin_rollback` | 回到上一个健康代际 |

工具描述要写清楚：新部署的代际先处于试用期，前几次调用失败会自动回退；`plugin_status` 可以看到每个代际的状态、失败原因和回退目标。描述中的试用次数 `TRIAL_CALLS` 由常量生成。

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
