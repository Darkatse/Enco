# 11 代际与注册表（enco-kernel `registry.rs`）

本章定义插件的身份、制品、两种代际、注册表、视图、准入与健康门控。它们都围绕**代际**展开，其余的要么是代际的属性，要么引用代际。代际、健康状态与接线都归注册表所有，注册表是唯一的提交者（架构文档 §3.6）。

## 1. 四样东西

| 名称 | 是什么 | 存在哪里 |
|---|---|---|
| 制品 | 不可变的组件文件，按内容哈希寻址 | `$ENCO_HOME/.data/artifacts/<哈希>.wasm`，经 Store 的 `put_artifact` / `artifact` 读写（03 §3.4） |
| 插件代际 | 一份已登记的插件实现：`(编号, 插件身份, 制品哈希, 插件配置, 来源, 状态, 失败原因)` | `enco.db` 的 `plugin_generations` 表（03 §3.2） |
| 配置代际 | 一份已登记的配置：`(编号, 文档)`。文档是 `config.toml` 的全部表（08 §3.1）；`[plugin.*]` 现在没有归属者，按通用规则拒绝，M23 起拆为插件代际 | `enco.db` 的 `config_generations` 表（03 §3.2） |
| 调用 | 一次导出函数调用，独占一个 Store，到返回为止 | 07 §3.3 |

两种代际的编号取自同一个全局自增的序号，所以一个编号就能定位一个代际，不需要插件名，也不需要说明是哪一种。

代际 = 内容 + 从内容求出的声明（架构文档 §4.5）：库里只存内容。插件代际的声明是加载时 `describe` 的结果（`Loaded`，04 §2），配置代际的声明是接线（§6）。插件配置是 `describe(config)` 的输入；插件设置尚未实现（§10），所以它恒为 `{}`。

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

注册表不检查 `~/.enco/plugins/<名字>/` 目录是否存在；`scaffold`、`rename`、`remove` 见 §10。

## 3. 注册表的数据

表定义在 03 §3.2：

- `generations`：两种代际共用的编号与创建时间；种类由代际在哪张子表里决定。
- `plugin_generations`：插件身份、制品哈希、插件配置、来源（`factory` / `deployed`）、状态（`trial` / `healthy` / `failed`）、失败原因（只有 `failed` 的代际有）。记录只追加，状态与失败原因一起更新。
- `config_generations`：文档，JSON。没有状态：配置代际登记即为健康（架构文档 §4.6）。
- `plugins`：插件身份、活跃插件代际的编号（可以为空）。
- `config`：只有一行，活跃配置代际的编号。

规则：

- 每个插件任一时刻至多一个活跃代际；节点任一时刻恰好一个活跃配置代际。
- **配置的回退目标**：编号小于活跃配置代际的配置代际中编号最大的那个。
- 活跃代际的状态是 `trial` 或 `healthy`。`failed` 的代际永远不再活跃；要再用这份制品，就重新部署，得到新的编号。
- **回退的候选**：同一插件中，编号小于当前活跃代际、状态为 `healthy` 的代际，按编号从大到小；最后是当前二进制的出厂代际（它不在前面、也不是当前活跃代际时）。出厂是恢复层级的底（架构文档 §4.6）：主版本升级后，旧制品全部无法加载，插件仍能落到当前的出厂代际，尽管它的编号比活跃代际大。**回退目标**是候选中的第一个；没有候选就没有回退目标，例如插件没有出厂代际且已处在最早的健康代际，或者活跃的就是当前出厂代际且没有更早的健康代际。
- **出厂代际**：来源为 `factory`，制品是当前二进制里嵌入的那一份。宿主升级后制品哈希变了，就是一个新的出厂代际；旧的出厂代际仍然是普通的健康代际。

## 4. 归属者：`Registry`

```rust
pub struct Registry { /* 见下 */ }

pub struct RegistryDeps {
    pub store: Arc<dyn Store>,
    pub runtime: Arc<dyn Runtime>,          // 04 §2：执行插件代际的声明函数
    pub factory: Vec<FactoryPlugin>,
    pub owners: Vec<TableOwner>,            // 宿主一侧的归属者：配置代际的声明函数（架构文档 §4.11）
    pub bootstrap: Result<serde_json::Value, String>,   // config.toml 转成的文档，或读不到、解析不了的原因（08 §3.1）；只在还没有配置代际时使用（§4.1）
    pub clock: Arc<dyn Clock>,
}

/// 嵌入宿主二进制的出厂插件；`id` 是宿主写死的身份。
pub struct FactoryPlugin { pub name: String, pub id: PluginId, pub artifact: Vec<u8> }

/// 一个宿主一侧的归属者：它拥有哪些表，以及从这些表求出的接线。
/// `declare` 解析自己的表，返回解析结果的接线（`settings.uses()`），不发调用。
pub struct TableOwner {
    pub tables: &'static [&'static str],
    pub declare: fn(&serde_json::Value, &KernelSettings) -> Result<Vec<Use>, SettingsError>,
}

/// 部署什么：新的组件字节，或重新登记某个已有插件代际的内容（撤销，架构文档 §4.6）。
pub enum PluginSource { Artifact(Vec<u8>), Generation(GenerationId) }

/// 应用什么：一份文档，或重新登记某个已有配置代际的文档（撤销，架构文档 §4.6）。
pub enum ConfigSource { Document(serde_json::Value), Generation(GenerationId) }

impl Registry {
    pub async fn open(deps: RegistryDeps) -> Result<Arc<Registry>, RegistryError>;
    /// 当前发布的视图。每次读取得到一份不可变的快照。
    pub fn view(&self) -> Arc<View>;
    /// 每次发布后收到新的视图；Scheduler 与渠道用它跟随配置（04 §10、10 §6）。
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<Arc<View>>;
    pub async fn deploy(&self, name: &str, source: PluginSource) -> Result<Deployed, RegistryError>;
    /// 校验并登记一份配置：与 `deploy` 对称，求出声明、探测与提交都在这里（§5）。
    pub async fn apply_config(&self, source: ConfigSource) -> Result<Applied, RegistryError>;
    /// 意图文件与已登记内容的关系（§4.3）。`document` 是宿主读取 config.toml 的结果，失败时是原因。
    pub async fn config_divergence(&self, document: Result<serde_json::Value, String>) -> ConfigDivergence;
    /// Round 开始时在提交者的锁内一起读取 Session 记录与视图，profile 引用与配置因此来自同一个边界（04 §6.2）。
    pub(crate) async fn session_view(&self, session: SessionId) -> Result<(SessionRecord, Arc<View>), RegistryError>;
    /// 改变 Session 选用的 profile。它也是接线，与应用配置由同一把锁排序（§5）。
    pub async fn set_profile(&self, session: SessionId, profile: &str) -> Result<(), RegistryError>;
    pub async fn rollback(&self, name: &str) -> Result<GenerationRecord, RegistryError>;
    pub async fn rollback_config(&self) -> Result<ConfigGeneration, RegistryError>;
    /// 调用方结算之后回报一次（§7）。`session` 接收可能产生的回退通知；返回值非空说明这次回报触发了回退。
    pub async fn report(&self, generation: GenerationId, verdict: Verdict, session: Option<SessionId>) -> Result<Option<RolledBack>, RegistryError>;
    pub async fn status(&self) -> RegistryStatus;
}

/// 导出表与活跃配置总是一同发布，所以读者看到的两者一致。
pub struct View { pub exports: Exports, pub config: ActiveConfig }
/// 活跃配置代际：已登记的内容，加上当前宿主从它求出的声明。只由一次求值构造，各部分不会各自变化。
pub struct ActiveConfig {
    pub generation: GenerationId,
    pub document: serde_json::Value,
    pub kernel: KernelSettings,           // 12 §1
    pub uses: Vec<Use>,                   // 内核与各归属者声明的接线（§6）
    pub invalid: Vec<SettingsError>,      // 当前宿主解析不了的表；有意的登记之后总为空（§4.1）
}

pub struct Deployed { pub generation: GenerationRecord, pub users: Vec<String> }
pub enum Applied { Registered { generation: GenerationId, previous: GenerationId, probes: Vec<Probe> }, Unchanged(GenerationId) }

/// 应用配置时通过的真实调用（§6）；失败探测直接拒绝登记。
pub struct Probe { pub user: String, pub endpoint: String }
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
    pub unavailable: Vec<String>,           // 当前活跃代际无法满足的使用者与原因；机械恢复允许留下它们
    pub generations: Vec<GenerationRecord>,   // 按编号升序
}

pub struct RegistryStatus { pub plugins: Vec<PluginStatus>, pub config: ConfigStatus }
pub struct ConfigStatus {
    pub active: GenerationId,
    pub rollback_target: Option<GenerationId>,
    pub generations: Vec<(GenerationId, DateTime<Utc>)>,   // 按编号升序
    pub invalid: Vec<SettingsError>,        // 活跃配置代际中当前宿主解析不了的表（§4.1），通常为空
}
```

注册表的内部状态包括：名字到身份的映射（经 Store 读自 `plugins.lock`）、全部代际记录、活跃编号映射 `BTreeMap<PluginId, GenerationId>`（没有键即没有活跃代际）、`ActiveConfig`、已加载的代际（`Loaded`，04 §2）、试用计数（§7），以及发布视图的 `watch::Sender<Arc<View>>`。除这个发布口外，这些状态都放在一个 `tokio::sync::Mutex` 里。这把锁就是唯一的提交者：读状态、检查前置条件、写数据库、重建并发布视图，都在持锁时按顺序完成。加载制品与应用配置的校验不持锁（§5）。

内存里只保留两类已加载的代际：每个插件的活跃代际，以及当前二进制的出厂代际。提交之后不再属于这两类的代际随即丢弃；还在进行的调用持有自己的 `Arc`，照常完成（架构文档 §4.5）。回退目标到需要时才加载。

### 4.1 启动

`Registry::open` 先检查各归属者的表互不重叠（否则返回 `TableOwner`），然后按顺序做七件事，错误带上插件名或表名：

1. 用 `store.plugin_names()` 读出名字与身份，核对出厂插件的身份（§2）。
2. 读注册表。如果某个代际引用的插件身份没有在 `plugins.lock` 登记，返回 `UnknownPlugin`。
3. 登记出厂制品：每份出厂制品写入制品库。还没有这个插件、这个哈希的出厂健康代际时，插入一条状态为 `healthy` 的出厂代际。插件没有活跃代际，或者活跃代际本身也是出厂代际时，把当前二进制的这条出厂代际设为活跃；主人自己部署的活跃代际保持不变。
4. 加载每个插件的活跃代际，以及第 3 步登记的当前出厂代际（安全模式要用，§7）；两者是同一个时只加载一次。出厂代际加载失败说明宿主自身坏了，拒绝启动。活跃代际加载失败时，按 §5 的激活流程回退到回退目标，把失败的代际标为 `failed` 并保存加载失败的原因，然后继续启动。
5. 配置：
   - 有活跃配置代际：对它的文档求出声明（§5）。内核的表解析不了就返回 `Config` 并拒绝启动；宿主一侧的表解析不了或没有归属者，记入 `invalid`，节点照常启动（架构文档 §4.5）。`bootstrap` 不使用。
   - 没有活跃配置代际：用 `bootstrap` 的文档登记第一个配置代际，直接为健康，不做真实调用（架构文档 §4.5）。`bootstrap` 是错误时返回 `Bootstrap`；求出声明或接线检查不通过时返回 `Rejected`。
6. 接线检查（§6），按 §6 的规则：首次登记不通过就拒绝启动，还原只报告。
7. 发布第一份视图。

前三步中的错误，以及第 4、5 步写明的情况，都拒绝启动。

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
- 每次提交后，在锁内重建一份新的视图，用 `send_replace` 发布（没有订阅者时也成功）。`view()` 是 `borrow().clone()`，取得一份不可变的视图后立即释放借用，用完即弃。导出表的读者是 Attempt（04 §6.4）、记忆的插件调用（06 §3.3）和 `plan::resolve`（04 §6.5），它们每次调用都重新解析；配置的各个读者在自己的边界读取（架构文档 §4.5）。

### 4.3 意图与已登记内容的不一致

`config_divergence` 在提交者的锁内，把意图文件的文档与活跃配置、历史比较：

- 相同：`None`。
- 与某个已登记、但不是活跃的配置代际 N 相同（有多个时取编号最大的）：`Superseded { generation: N, reason }`。原因从代际顺序推出：活跃代际的编号小于 N，是 `enco config rollback` 回到了更早的代际；大于 N，是之后又登记了别的内容，其中活跃代际的文档与更早的某个代际 M 相同时，写明它重新登记了 M。
- 与所有已登记的配置代际都不同，或者读取失败：`Unapplied { reason }`，读取失败时带上原因。

## 5. 部署、应用配置与回退

本节说的"拒绝时什么都不写"，指不登记、不激活候选内容；已经发生的配置探测仍向健康门控回报（§7）。

活跃代际只有两种改法：登记一个新代际（部署、应用配置），或者激活一个已有的代际（回退）。登记要先校验，加载制品要几十毫秒，真实调用可能要几秒，所以校验都在锁外进行，在锁内提交。登记从活跃代际继承了一部分，所以提交时核对**期望的活跃代际**：校验所依赖的那几个活跃代际的编号必须仍是活跃的，否则返回 `Conflict`，什么都不写，重新执行即可（架构文档 §3.6 规则二）。试用晋升不改变活跃代际，不算冲突。

`deploy(name, source)` 分四步：

1. 记下这个插件的活跃代际与活跃配置代际，作为期望。`PluginSource::Artifact` 把字节写入制品库（按内容寻址，幂等，不需要锁），插件配置取活跃代际的；`PluginSource::Generation(n)` 取代际 `n` 的全部内容（制品与插件配置），`n` 不是这个插件的代际时返回 `Rejected`，不存在时返回 `UnknownGeneration`。
2. 用 `runtime.load(制品, 插件配置)` 加载。制品不是组件、导入不满足（WIT 不匹配）或 `describe` 失败时，返回 `Rejected`，原样带出错误信息。
3. 调用 `probe()`（§7）。失败时返回 `Rejected`，不写任何记录。
4. 持锁提交：核对第 1 步的期望；名字还没有登记就先登记（§2）；按活跃配置代际的接线做准入检查（§6）；插入状态为 `trial` 的新代际并设为活跃，然后重建并发布视图。

两个部署并发时，后提交的那个发现活跃代际已经变了，返回 `Conflict`。同一份制品再次部署会得到新的编号，不做去重；只有出厂代际在启动时去重。重新登记旧代际的内容同样进入试用，历史只向前追加。

**求出声明**是配置代际的声明函数，只求事实，不决定拒绝还是继续：

1. 内核解析自己的表，失败时返回带表名的 `SettingsError`。
2. 文档中没有归属者的表记入 `invalid`。
3. 各归属者求出接线，失败记入 `invalid`，成功的接线与 `kernel.uses()` 合并。

结果与代际记录一起构成 `ActiveConfig`，这是它唯一的构造方式。有意的登记（首次登记、应用配置）拒绝任何错误或 `invalid`；还原与 `rollback_config` 把内核的错误映射为 `Config`，保留 `invalid`。

`apply_config(source)` 与 `deploy` 对称，分四步。调用方只负责给出文档或代际编号（08 §3.1）。

1. 取当前的视图，后面的期望都从它取。`ConfigSource::Document` 就是要登记的文档；`ConfigSource::Generation(n)` 取配置代际 `n` 的文档，`n` 是插件代际时返回 `Rejected`，不存在时返回 `UnknownGeneration`。文档与活跃配置代际的相同时返回 `Unchanged`，不登记。
2. 求出声明，失败时返回 `Rejected`。期望是视图中的活跃配置代际，以及候选接线所用插件的活跃代际。
3. 锁外探测：候选接线中的每个使用者，只要它的（插件, 参数, 需要）组合没有出现在活跃配置代际的接线中，就按 §6 的规则做一次最小调用。任何一次失败都返回 `Rejected`，写明使用者、endpoint 与失败原因。
4. 持锁提交：
   - 核对第 2 步的期望。
   - 按候选的接线做准入检查（§6）。
   - 每个 Session 的 profile 都在候选中，否则返回 `Rejected` 并列出这些 Session（架构文档 §4.10 第 3 条）。
   - 在一个事务里插入配置代际并设为活跃，然后重建并发布视图，返回 `Registered` 与第 3 步的探测记录。

`set_profile(session, name)` 持锁：名字不在活跃配置代际的 profile 中就返回 `UnknownProfile`，否则经 Store 写入 `sessions.profile`（03 §2）。它与应用配置第 4 步由同一把锁排序；Session actor 在每个 Round 开始时经 `session_view` 读取这条记录（04 §6.2）。

**激活**是手动回退、自动回退（§7）和启动恢复（§4.1）共用的流程，形式为 `activate(插件, expected, 候选, 原因)`。原因是手动回退、启动恢复、试用失败三者之一，试用失败还带着故障和来源 Session。调用方先持锁确定当前活跃代际（即 expected）和候选，然后：

1. 锁外准备：按 §3 的顺序取候选作为目标，还没加载就加载它。加载失败时记下这个代际及其原因，改用下一个候选，直到候选用尽。
2. 持锁核对：活跃代际已经不是 expected、目标已被标为 `failed`，或者试用失败时 expected 已经晋升为 `healthy`，都返回 `Conflict`，本次准备的写入一律不提交。
3. 在同一个事务里：把要标为失败的代际（包括第 1 步加载失败的）连同原因标为 `failed`，更新活跃代际；试用失败有来源 Session 时，把回退事件写入它的 Inbox。事务成功后更新内存，发布视图。

第 2 步就是架构文档 §3.6 规则二：准备期间有人部署了新代际，或者试用代际已经晋升，这次激活就作废。手动回退遇到 `Conflict` 时把错误返回给主人，重试即可。自动回退遇到时直接忽略，因为原来的试用条件已经改变，这和忽略迟到的回报是同一条规则。

`rollback_config()` 持锁取出活跃配置代际与 §3 的配置回退目标，没有目标就返回 `NoRollbackTarget`；有目标就像还原那样对它求出声明，然后设为活跃并发布视图（内核的表解析不了时返回 `Config`，活跃代际不变）。它是机械恢复，不做准入检查（§6）。

`rollback(name)` 持锁取出当前活跃代际和 §3 的回退目标，没有目标就返回 `NoRollbackTarget`；有目标就激活它，调用方自己不要求标记任何代际。当前代际的状态保持不变：它没有坏，只是主人不想用。如果所有候选都加载失败，就提交这些失败，保留当前活跃代际，返回 `NoRollbackTarget`。启动恢复和自动回退则不同，候选用尽时允许插件没有活跃代际。

只有制品缺失、损坏或 Runtime 加载失败，才能判定代际不可用；统一记录为 `plugin.load`，message 保留原始错误。底层存储故障直接向上传播，不会因此把代际标为失败。

## 6. 准入：接线

接线是"谁使用哪个 endpoint，需要它提供什么"，是配置代际的声明（§5）。接口、凭证是否匹配与应用配置时的探测，都由注册表从同一份声明推出：

| 使用者 | 归属者 | 需要 |
|---|---|---|
| `profile <名字>.reply` | 内核（12 §1） | `Completion { tools: true }` |
| `profile <名字>.compaction` | 内核（12 §1） | `Completion { tools: false }` |
| `memory.embedding` | 记忆（06 §7） | `Embedding { dimensions }`，取自所选 endpoint |
| `memory.relevance` | 记忆（06 §5.1，写了时） | `Decision` |

```rust
pub struct Use {
    pub user: String,               // 形如 "profile default.reply"、"memory.embedding"
    pub endpoint: Endpoint,         // 名字、插件和参数（含 api_key_env），共用 12 §1 的类型
    pub need: Need,                 // 接口由它推出，也是探测的内容
}
pub enum Need { Completion { tools: bool }, Embedding { dimensions: NonZeroUsize }, Decision }
```

**准入**只有一个函数：给定插件和一份 `Loaded`，确认这个插件的每个使用者需要的接口都在导出中（`Completion` 要 `completion`，`Embedding` 要 `embedding`，`Decision` 要 `decision`），并且凭证与使用者相符。密钥写在决定请求去向的 endpoint 旁边（架构文档 §4.11），所以凭证在接线中检查，不在代际的准入中：

- 使用者的 `settings.api_key_env` 有值，插件却没有声明凭证（`Loaded.credential` 为空）：这个 key 无处可放。
- 插件声明凭证为必需，使用者没有给 `api_key_env`。

两者都不通过。同一个插件代际可以同时服务带 key 与不带 key 的 endpoint，只要它声明的凭证不是必需的。插件未登记或没有活跃代际，同样不通过。

检查的结果只按命令的性质处理：

- **有意的命令**（首次登记、应用配置、部署）不通过就返回 `Rejected`，错误列出使用者与原因，什么都不登记；它们不能拆掉在用的接线。
- **机械恢复**（启动还原、`rollback`、`rollback_config`）不因此失败：还原把不匹配的使用者记入日志与 `PluginStatus.unavailable`。之后缺接口的调用在 `Exports` 处得到 `plugin.unavailable`；凭证不相符的调用照常进行，由服务给出结果（07 §3.3）。

**探测**（§5 第 3 步）按需要做一次最小调用，只用宿主固定的文本，不带任何 Session 的内容，所以不属于任何 Session；发了什么、结果如何随 `Applied` 交给调用方，经工具应用时就在 Agent 的 Log 中（架构文档 §4.6）：

- `Completion { tools }`：一条用户消息 `CONFIG_PROBE_PROMPT`，输出上限 `CONFIG_PROBE_MAX_TOKENS`；`tools` 为真时带一个工具定义 `CONFIG_PROBE_TOOL`。返回任何补全都算通过，不看内容。
- `Embedding { dimensions }`：嵌入 `CONFIG_PROBE_PROMPT`，向量宽度必须等于 `dimensions`。
- `Decision`：以 `CONFIG_PROBE_PROMPT` 为 state 问一个 Predicate 问题，任何答案（包括 `refused`）都算通过。

调用经各自的端口，用活跃代际的导出；变量缺失时以 `credential.unavailable` 失败（07 §3.3）。

## 7. 健康门控

健康只统计能归因于代际本身的失败：

| 失败码 | 含义 | 谁产生 |
|---|---|---|
| `plugin.trap` | wasmtime 在实例化或调用中报错：trap、资源超限、导出缺失 | enco-wasm（07 §3.3） |
| `plugin.contract` | 插件的返回违反契约：角色不对、JSON 字段无法解析、向量数量或维度不对 | enco-wasm 的转换 |
| probe 失败 | `lifecycle.probe` 返回错误 | 部署时（§5） |

`provider.*`、`timeout`、`cancelled` 是外部失败或主人的操作，在任何代际上都可能出现，不计入。

- **回报**：凡是调用了插件导出的地方，都在结算之后调用一次 `report(代际编号, verdict, 来源 Session)`：内核的 Attempt（04 §6.4）、记忆的插件调用（06 §3.3）与配置探测（§6）。Attempt 传入本 Session，其余传入 `None`。回报带着代际编号，所以迟到的回报碰不到更新之后的代际。verdict 只看插件调用本身：回复或摘要无法使用、向量维度与索引配置不符，是调用方自己的判断，不算插件失败。
- **处理**（持锁进行）：
  - 代际已经不是所属插件的活跃代际：忽略。
  - 代际是 `healthy`：忽略（健康代际不降级，§10）。
  - 试用代际调用成功：成功次数加一，达到 `TRIAL_CALLS` 时晋升为 `healthy`。
  - 试用代际出现可以归因于它自身的失败：立即回退。以这个代际为 expected 激活回退目标，并以这次的 `Failure` 为原因把它标为失败（§5），成功则返回 `RolledBack`；没有回退目标时，插件不再有活跃代际。一次失败就回退，不设阈值。
  - 试用代际出现不能归因于它的失败：忽略。
- 成功次数按代际累计，各接口、不同 Session 与 endpoint 共用计数。外部失败既不增加也不清零；计数只记在内存里，重启后从零开始。
- **回退通知**：回报带有来源 Session 时，注册表在回退的同一事务里向它的 Inbox 投递 `EventBody::GenerationRolledBack`，来源是 `EventSource::Registry`（03 §1.4），通知文字由注册表此时写入 Event 的 `text`。这就是架构文档 §4.6 所说的 `deploy.rolled_back`。模型在下一个 Round 看到它；回退后进程立即崩溃，事件也会在重启后消费。Attempt 收到 `RolledBack` 后按 04 §6.4 立即用新代际重试。通知只发给出故障的 Session；其他 Session，包括部署它的那个，从 `plugin_status` 查看失败原因。
- **probe**：`lifecycle.probe()` 只做自检，不联网，也不依赖主人的配置。出厂的 Provider 插件直接返回成功。它的作用是让实例化之后就无法使用的制品在部署时被拦下，而不是等到主人下一句话时才暴露。
- **安全模式**：`Exports::completion(插件, safe_mode = true)` 返回出厂代际的导出（§4.2）。没有出厂代际的插件在安全模式下仍用活跃代际，没有更好的选择。

## 8. 命令与工具

CLI 与 Agent 工具同源，都调用 `Registry` 的方法。`Registry::deploy` 只接收字节或代际编号，文件由调用方读取：CLI 这边是守护进程（08 §6），Agent 这边是宿主的插件工具（05 §2.6）。内核因此不做文件 IO。

| CLI | 工具 | 作用 |
|---|---|---|
| `enco plugin status` | `plugin_status` | `[PluginStatus]` |
| `enco plugin deploy <name> (<path> \| --generation <N>)` | `plugin_deploy` | 部署一个组件文件，或重新登记某个已有代际的内容 |
| `enco config apply [--generation <N>]` | `config_apply` | 应用 `config.toml`，或重新登记某个已有配置代际的文档（08 §3.1） |
| `enco plugin rollback <name>` | — | 回到回退目标 |
| `enco config rollback` | — | 回到上一个配置代际 |

两个 `rollback` 是主人的机械恢复，只在 CLI 中；Agent 的撤销是以编号重新登记（架构文档 §4.6）。工具描述写明试用期与自动回退、以编号重新部署就是撤销，以及 `plugin_status` 能查到什么；试用次数由 `TRIAL_CALLS` 生成。

构建由主人自己完成（`plugin_build` 见 §10）：在 `~/.enco/plugins/<name>/` 里 `cargo build --release --target wasm32-wasip2`，然后把产物路径交给 `deploy`。出厂插件依赖 `provider-protocol`，复制出来的目录用 path 或 git 依赖指向 Enco 仓库即可。

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
    #[error("{name} rejected: {reason}")]
    Rejected { name: String, reason: String },  // 加载、probe、准入或候选配置不通过；name 是插件名，配置本身不通过时为 "config"
    #[error("unknown generation {0}")]
    UnknownGeneration(GenerationId),
    #[error("unknown profile {0}")]
    UnknownProfile(String),                     // set_profile 的名字不在活跃配置代际中；Kernel 转为 KernelError::UnknownProfile
    #[error("{0} has no generation to roll back to")]
    NoRollbackTarget(String),                   // 插件名，或 "config"
    #[error("{0}: the registry changed while preparing; retry")]
    Conflict(String),
    #[error("configuration: {0}")]
    Config(SettingsError),                      // 已登记的内核表解析不了（§4.1）
    #[error("initial configuration unavailable: {0}")]
    Bootstrap(String),
    #[error("table {0} has multiple owners")]
    TableOwner(String),                         // 组合根交给注册表的归属者重叠；在 open 时检查一次
}
```

```rust
// enco-kernel limits.rs
pub const TRIAL_CALLS: u32 = 5;
pub const CONFIG_PROBE_PROMPT: &str = "Reply with the single word OK.";
pub const CONFIG_PROBE_MAX_TOKENS: u32 = 16;
pub const CONFIG_PROBE_QUESTION: &str = "Does the state ask for the word OK?";
// CONFIG_PROBE_TOOL：一个名为 `probe` 的工具，没有参数，描述写明不需要调用它
```

## 10. 尚未实现

插件设置与插件自带的凭证（M23）；工具接口、Round 钉住导出表、`completion` 导入方的检查与 `generation_revoked`（M23）；构建命令与 lint、`scaffold` / `rename` / `remove`（M24）；运行时手册与 README 生成区（M25）；状态接口（P3）；制品的垃圾回收；健康代际的自动降级；流式补全；跨节点。
