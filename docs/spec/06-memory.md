# 06 记忆（enco-host `memory/`）

记忆是关于主人的长期信息：偏好、人物、约定、重要的事。它是宿主提供的能力，经内核已有的两个端口接入：写入是工具（Tool），召回是上下文源（ContextSource）。**内核不知道记忆的存在**，也不为它增加任何概念。

## 1. 结构与三条规则

```text
   memory_save / update / forget / search              召回（有新输入时，经 ContextSource）
                     │                                          │
                     ▼                                          ▼
   ┌───────────────────────── Memories（归属者）─────────────────────────┐
   │   权威：memory.db（SQLite）   ──────对账──────▶   索引：TriviumDB（派生） │
   └────────────────────────────────────────────────────────────────────────┘
                                        │ embed
                                        ▼
            Embedding 端口（[memory] embedding 所选 endpoint 的插件的活跃代际，11 §4.2）→ Wasm 插件 → /embeddings
```

全部机制都从下面三条规则推出。遇到规格没有写到的情况，先回到这三条。

1. **归属**：记忆跨 Session 共享，是可变状态，由唯一的归属者 `Memories` 管理（架构文档 §3.6 规则一）。它与 Scheduler 管理提醒是同一个模式：主人的 CLI 命令与 Agent 的工具调用走同一组方法。
2. **权威与派生**：`memory.db` 是权威。TriviumDB 索引是权威与 embedding 身份的函数：每次召回前，都用同一个对账函数比较两者并补齐差异，所以不需要在任何地方记住"哪些还没进索引"。对索引有任何疑问（打不开、视图中 embedding 的身份与索引不同），就删掉重建，不做修补。
3. **索引提名，权威裁决**：检索命中的每一条，都回到权威读取当前内容之后才能返回；权威中已经不存在的命中直接丢弃。因此过期的索引只影响召回质量，永远不会让已经更正或遗忘的内容重新出现。

## 2. 权威：memory.db（`memory/authority.rs`）

- 文件：`$ENCO_HOME/.data/memory.db`，与 `enco.db` 分开：一个归属者，一个文件。
- 连接方式与 Store 相同（03 §3.1）：一个 `rusqlite::Connection` 放在 `std::sync::Mutex` 中，在 `spawn_blocking` 中执行；`PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;`；写事务使用 `BEGIN IMMEDIATE`。
- 版本规则与 `enco.db` 相同（03 §3.1）：空库建表并把 `PRAGMA user_version` 设为 1；已有库的版本不是 1 时返回 `MemoryError::SchemaVersion`，拒绝启动。

```sql
CREATE TABLE memories (
  id         TEXT PRIMARY KEY,         -- MemoryId（ULID）
  text       TEXT NOT NULL,
  pinned     INTEGER NOT NULL CHECK (pinned IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  rev        INTEGER NOT NULL          -- 创建时为 1，每次修改加 1
) STRICT;
```

- **遗忘就是删除这一行。** 权威中不保留被遗忘的内容。它仍然留在当时那个 Session 的 Log 中（Log 永不改写），这是 Log 的性质，不是记忆的。
- `rev` 只用于判断索引中的那一份是否最新（§3.3），不表示记忆之间的顺序。
- 类型 `Memory` 与 `MemoryId` 定义在 enco-core（03 §1.10）。

## 3. 索引：TriviumDB（`memory/index.rs`）

### 3.1 形态

- 目录 `$ENCO_HOME/.data/memory-index/`，其中是 `memory.tdb`（以及 TriviumDB 自己的附属文件）和 `index.json`：建索引时的身份 `{"base_url", "model", "options", "dimensions"}`（§7）。
- 打开：`Database::<f32>::open_with_config(path, Config { dim: dimensions, load_text_index: false, ..Default::default() })`，其余保持默认。**不要**调用 `enable_auto_compaction`（它会启动后台线程）。
- 每条记忆对应一个节点：向量是记忆文本的 embedding，payload 是 `{"memory": "<MemoryId>", "rev": n}`。节点 ID 由 TriviumDB 分配，只在索引内部有意义，不出现在索引之外。
- 内存中维护 `nodes: HashMap<MemoryId, IndexedMemory>`，`IndexedMemory { node, rev }` 明确区分索引节点与权威修订号，打开时从 payload 重建。
- **替换一条记忆 = 删除旧节点 + 插入新节点 + `index_text(新节点, 文本)`。** 不在原节点上更新向量或重新 `index_text`：一种写法，语义最清楚。一批操作结束后调用一次 `build_text_index()`。
- TriviumDB 的 API 是同步的，全部调用放在 `spawn_blocking` 中。

打开流程由 TriviumDB 0.8.8 的两个已核实行为决定：WAL 只记录节点与边，崩溃后节点可以恢复；文本索引只在内存中，不写 WAL。所以每次打开都用权威中的文本重建文本索引，而不是依赖它自己的持久化。

### 3.2 打开与对账

索引的身份是视图中 `[memory] embedding` 所选 endpoint 的 `identity()`（§7）。`sync`（§3.3）每次先取当前视图的身份；与已打开索引的身份不同，或者还没有打开索引时，按下面的流程打开：

```text
open_index(identity):
    if index.json 不存在，或与 identity 不一致:
        删除整个目录，重新创建，写入 index.json
    db = 打开 memory.tdb；被另一个进程占用时返回 MemoryError::Index；其他失败删除目录并重建一次，再失败返回 MemoryError::Index
    active = 权威中全部记忆的 (id, rev, text)
    r = reconcile(active 的 (id, rev), 索引中每个节点的 (NodeId, 解析出的 (MemoryId, rev) 或 None))
    删除 r.delete 中的节点；nodes = r.keep
    for 每个保留的节点: index_text(节点, 权威中的文本)
    build_text_index(); flush()
```

`Memories::open` 打开权威，然后重复 `sync(None)`，直到 unindexed 为空、没有进展或某一次失败；第一次 `sync` 就打开了索引。所以启动与改了 `[memory] embedding` 走同一条路：身份变了，索引删掉重建，全部记忆按批重新进入索引，期间它们按 §4 直接出现在召回结果中。

`flush()` 只在打开索引时调用：运行期间的写入留在 TriviumDB 的 WAL 中，下次打开时由它自己回放。

embedding 服务不可用时照常打开，未进入索引的记忆在召回时直接纳入（§4）。守护进程不因此拒绝启动。

`[memory]` 在当前视图中解析不了（11 §4.1）时，`sync` 不改变已打开的索引，不发 embedding 请求，以 `config.invalid` 失败，召回退化为只按文本检索；还没有打开过索引时，召回只返回未进入索引的记忆。

### 3.3 对账（`reconcile` 与 `sync`）

`reconcile` 是纯函数，打开时与运行中共用：

```text
reconcile(active: [(MemoryId, rev)], nodes: [(NodeId, Option<(MemoryId, rev)>)]) -> { delete, keep, unindexed }
```

一个节点被保留，当且仅当它的 payload 可以解析、`active` 中有同 id 同 rev 的记忆、并且这条记忆只有这一个节点；其余节点进入 `delete`。`active` 中没有被保留节点的记忆进入 `unindexed`。打开时 `nodes` 来自扫描 payload；运行中来自内存中的 `nodes`。

运行中，写入只改权威，不触碰索引，也不留下任何标记。每次召回都先执行一次 `sync`（持有索引锁），与召回共用一次 embedding 请求：

```text
sync(query: Option<&str>, cancel) -> Result<Synced, MemoryError>
    settings = 当前视图的记忆设置（§7）；解析不了时按 §3.2 以 config.invalid 结束
    if 索引没有打开，或身份与 settings.embedding 的不同: open_index(身份)（§3.2）
    endpoint = settings.embedding
    active = 权威中全部记忆的 (id, rev)，由权威查询按 updated_at 从新到旧、id 打破同时间并列
    r = reconcile(active, nodes)
    删除 r.delete 中的节点
    batch = r.unindexed 中按 updated_at 从新到旧的前 EMBED_BATCH - 1 个
    rows = 权威中 batch 的当前内容（期间被遗忘的跳过）
    inputs = [query（如果有）] ++ rows 的文本
    if inputs 为空: return Synced { query_vector: None, unindexed: [], failure: None }
    结果 = reported_call(exports.embedding(endpoint.plugin), embed(&endpoint.settings, inputs))
    成功，且向量维度与索引配置相符:
        替换每个 row 的节点，节点的 rev 取自 row；build_text_index()
        return Synced { query_vector, unindexed: r.unindexed 去掉 rows, failure: None }
    否则（失败，或目标维度不符，后者为 provider.bad_response）:
        return Synced { query_vector: None, unindexed: r.unindexed, failure }
```

`reported_call` 是记忆调用插件的唯一路径，相关性过滤（§5.1）也用它：导出按调用解析（11 §4.2），解析失败即 `plugin.unavailable`；调用可以被 cancel 打断，失败不重试；结束后向注册表回报（11 §7，只有 `plugin.trap` 与 `plugin.contract` 计入健康）；已取消就返回 `Cancelled`。

嵌入插件的新代际与 `[memory] embedding` 的新 endpoint，都从下一次 `sync` 起使用。回报触发回退时，注册表记下失败原因（11 §7）。记忆没有来源 Session，所以不投递通知，原因在 `plugin_status` 里查看。

数量、向量非空、维度彼此一致与数值有限属于 `Embedding` 的返回契约，由运行时边界核对（07 §3.3）。Memories 只核对索引配置的目标维度；配置不匹配不能据此把插件标为失败。

- **状态只在权威与索引中**：`sync` 被取消、失败或与写入并发时都没有需要恢复的东西，下一次对账会看到同样的差异。与 `sync` 并发提交的写入，由下一次召回补上。
- 代价是每次召回读取一遍按更新时间排序的 `(id, rev)`，O(记忆数)，个人规模下是毫秒级。
- 不重试。未进入索引的记忆由下一次召回再试；在此之前它们直接出现在召回结果中（§4）。

## 4. 召回

```text
Memories::recall(query: &str, limit: usize, cancel) -> Result<Recall, MemoryError>
    获取索引锁（等待可被 cancel 打断）
    s = sync(Some(query), cancel)?
    hits = search_hybrid(Some(query), s.query_vector, SearchConfig {
               top_k: limit, expand_depth: 0, min_score: 0.0,
               enable_text_hybrid_search: true, ..Default::default() })
    memories = 按命中顺序从权威读取，不存在的丢弃               // 索引提名，权威裁决
    unindexed = s.unindexed 中不在 memories 里的记忆，按 updated_at 从新到旧，至多 limit 条
    Recall { memories, unindexed, lexical_only: s.failure }
```

- 向量不可用时，TriviumDB 仍可只按文本检索（中文按两字切分计算 BM25，已实测）。
- 检索不设相似度阈值：相似度的尺度依赖具体的 embedding 模型，所以固定取前 `limit` 条；是否相关由可选的相关性过滤判断（§5.1）。
- 置顶记忆在 System 消息中，每个 Round 都从权威读取；召回的记忆写在它所回应的输入之前，之后原样保留（05 §4.1）。
- 自动召回只在有新输入的 Round 执行一次，以最新的已接纳 Event 为查询；一次接纳多条 Event 时也只召回一次。没有新输入时只提供置顶记忆，不请求 embedding。同一 Round 内的压缩重组沿用已取得的贡献（04 §6.3）。不缓存查询向量（01 §2），后续根据实际延迟与用量判断是否优化。
- 取消只打断两种等待：索引锁与 embedding 响应。已经开始的 `spawn_blocking`（SQLite、TriviumDB）等它结束，然后返回 `MemoryError::Cancelled`。

## 5. 上下文源：MemoryContextSource（`memory/context.rs`）

实现 `ContextSource`（04 §2）。每轮提供置顶记忆；仅当 `ContextQuery.new_input` 为真且 `latest_event` 存在时调用召回，查询文本是该 Event 的规范消息文本（`canonical_message().joined_text()`），取消令牌是 `ContextQuery.cancel`。

循环中保存或更正的记忆立即进入权威，置顶内容下一 Round 即可见；索引在下一次自动召回、显式 `memory_search` 或启动对账时更新。同步未完成的记录仍按 §4 作为 `unindexed` 返回。

贡献中的候选按下面的顺序排列。顺序就是优先级：composer 按顺序纳入，直到记忆预算用完（05 §4.2）。置顶保证优先级，不保证无条件纳入；超预算的置顶记忆也记录在 `ContextPlan.omitted` 中，不因此让 Round 失败。

1. 全部置顶记忆，按 id 顺序，`standing: true`。
2. `recall(query, MEMORY_RECALL_DEFAULT)` 的 `memories`，去掉置顶的，`standing: false`。
3. `recall` 的 `unindexed`，去掉置顶的，`standing: false`。

`[memory]` 写了 `relevance` 时，第 2、3 项先经过相关性过滤（§5.1）。

每个候选为 `Candidate { id: "memory:<MemoryId>", kind: Memory, text, standing }`。

`lexical_only` 存在时，贡献中带一条省略：`Omission { source: "memory:semantic", reason: "embedding failed ({code}): {message}; memories were recalled by keywords only" }`。它随计划写入 Log，所以这次降级是可见的，而不是静默的。

读取权威失败时返回 `ContextError`：记忆的权威不可用，这一轮就明确失败。被取消时也返回 `ContextError`，内核按令牌状态把这一轮结算为 `Cancelled`（04 §9）。

### 5.1 相关性过滤（`memory/relevance.rs`）

检索按相似度取前 `limit` 条，其中可能有与输入无关的记忆。`[memory]` 写了 `relevance`（§7）时，召回之后用一次 `decide` 判断每条记忆是否与输入相关。只过滤自动召回：`memory_search` 的结果由 Agent 自己判断，置顶记忆总是提供。

相关性过滤是 `Memories::filter_relevance`，由 `MemoryContextSource` 在召回后调用，与 `sync` 共用记忆的设置（§7）。

```text
filter_relevance(query, recalled, cancel)
    endpoint = 当前视图的记忆设置（§7）中的 relevance；设置解析不了时全部保留，记一条省略
    if endpoint 为空: return Filtered { kept: recalled, omitted: [] }      // 没有写 relevance，不过滤
    if recalled 为空: return Filtered { kept: [], omitted: [] }
    questions = 每条记忆一个 Predicate，instructions 为 RELEVANCE_QUESTION 填入记忆内容
    结果 = reported_call(exports.decision(endpoint.plugin), decide(&endpoint.settings, query, questions))   // 与 §3.3 的 embed 同一条路径
    成功: 概率低于 MEMORY_RELEVANCE_THRESHOLD 的记忆移出 kept，各记一条省略；Refused 的保留
    失败: 全部保留，记一条省略
```

state 是召回用的同一段查询文本。`RELEVANCE_QUESTION` 是面向模型的常量：

```text
Does this stored memory about the user relate to the message?
Yes if it concerns the message's topic, request, people or things; no if it is about something unrelated.

Memory: {记忆内容}
```

问的是"是否有关"而不是"是否有帮助"：误删一条需要的记忆，代价比多带一条边缘记忆大，所以只有讲无关之事的记忆才应低于阈值。

省略随计划写入 Log，`enco inspect` 可以核对每条记忆为什么这次没有加入：

- 判为无关：`Omission { source: "memory:<MemoryId>", reason: "judged irrelevant to the input (p={概率，两位小数})" }`。
- 判断失败：`Omission { source: "memory:relevance", reason: "decision failed ({code}): {message}; recalled memories were not filtered" }`。
- 有答案为 Refused：`Omission { source: "memory:relevance", reason: "decision refused {n} memories; they were kept" }`。
- 设置解析不了：`Omission { source: "memory:relevance", reason: "config.invalid: {错误}; recalled memories were not filtered" }`。

判断失败只让召回变宽，与 `lexical_only` 相同，Round 照常进行。

## 6. 工具（`memory/tools.rs`）

```rust
pub fn memory_tools(memories: Arc<Memories>) -> Vec<Arc<dyn Tool>>;
```

`code()` 为 `CodeRef::Native { name: "host-memory", version: crate 版本 }`。参数与执行错误的约定同 05 §2；ID 无法解析为 `tool.invalid_arguments`。

| 名称 | 参数 | 结果 | Effect |
|---|---|---|---|
| `memory_save` | `text`；`pinned`：布尔，可选，默认 false | `Ok { id }` | SideEffect |
| `memory_update` | `id`；`text` 与 `pinned` 至少给出一个 | `Ok { id }`；记忆不存在 → `tool.failed` | SideEffect |
| `memory_forget` | `id` | `Ok { forgotten: bool }` | SideEffect |
| `memory_search` | `query`；`limit`：1 到 `MEMORY_RECALL_MAX`，默认 `MEMORY_RECALL_DEFAULT` | `Ok { memories: [ { id, text, pinned } ], note? }` | ReadOnly |

- `text` 去掉首尾空白后必须非空，且不超过 `MEMORY_TEXT_BYTES`，否则 `tool.invalid_arguments`。
- 写入工具只调用 `Memories` 的方法，只改权威（§3.3）。
- `memory_search` 以 `CallContext.cancel` 调用 `recall`，返回 `memories` 加上 `unindexed`；`lexical_only` 存在时，`note` 说明本次只按关键词检索及原因。被取消时返回 `Failed { code: "cancelled" }`：它是只读的，没有发生任何事。
- `description` 写清什么值得记住。例如 `memory_save`：`Save a durable fact about the owner (a preference, a person, a commitment, an important event) as one self-contained statement. Include dates when the fact is time-bound. Set pinned only for facts that matter in almost every conversation, such as the owner's name.`

## 7. Memories 的接口（`memory.rs`）

```rust
pub struct Memories { /* 见下 */ }

pub struct MemoryPaths { pub db: PathBuf, pub index: PathBuf }
// 以下配置类型位于 memory/settings.rs，只供记忆模块内部使用。
/// [memory] embedding 所选的 endpoint：哪个插件、什么参数、什么维度。
pub(super) struct EmbeddingEndpoint { pub endpoint: Endpoint, pub dimensions: NonZeroUsize }

impl EmbeddingEndpoint {
    /// 索引的身份：决定向量空间的全部定义，即 `base_url`、`model`、`options` 与 `dimensions`。
    /// `options` 整体纳入，因为宿主不解读它，无法知道哪一项影响向量空间；多重建一次是安全的，混用两种向量空间不是。
    /// 不含 endpoint 的名字与 `api_key_env`，它们不改变向量；也不含插件，`embedding` 的契约是返回服务给出的向量，插件只翻译协议。
    pub fn identity(&self) -> serde_json::Value;
}

/// [memory] 的解析结果。两种使用者共用 `Endpoint`，按名字由 `KernelSettings::endpoint` 解析（12 §1）。
pub(super) struct MemorySettings { pub embedding: EmbeddingEndpoint, pub relevance: Option<Endpoint> }

impl MemorySettings {
    /// 解析 [memory]。`embedding` 必填，所选 endpoint 必须写了 `dimensions`；`relevance` 可选，所选 endpoint 必须存在。
    fn from_document(document: &serde_json::Value, kernel: &KernelSettings) -> Result<MemorySettings, SettingsError>;
    /// 接线（11 §6）：memory.embedding 声明 `Need::Embedding { dimensions }`，memory.relevance 声明 `Need::Decision`。
    fn uses(&self) -> Vec<Use>;
}

/// 交给注册表的归属者（11 §4）：`tables = ["memory"]`，`declare` 是 `from_document` 之后取 `uses()`。
pub const TABLE_OWNER: TableOwner;

pub struct Recall { pub memories: Vec<Memory>, pub unindexed: Vec<Memory>, pub lexical_only: Option<Failure> }
pub struct MemoryList { pub memories: Vec<Memory>, pub unindexed: Vec<MemoryId> }

impl Memories {
    /// 打开权威，并按 §3.2 完成第一次对账。endpoint、索引身份与嵌入适配器都在每次 sync 时从视图取（§3.3）。
    pub async fn open(paths: MemoryPaths, registry: Arc<Registry>, clock: Arc<dyn Clock>) -> Result<Arc<Memories>, MemoryError>;
    pub async fn save(&self, text: String, pinned: bool) -> Result<Memory, MemoryError>;
    /// 记忆不存在时返回 None。
    pub async fn update(&self, id: MemoryId, text: Option<String>, pinned: Option<bool>) -> Result<Option<Memory>, MemoryError>;
    pub async fn forget(&self, id: MemoryId) -> Result<bool, MemoryError>;
    pub async fn pinned(&self) -> Result<Vec<Memory>, MemoryError>;
    /// 全部记忆（置顶在前，其余按 updated_at 从新到旧）与尚未进入索引的 ID，供 CLI 使用。
    /// 未进入索引的 ID 由 reconcile 算出，不发起 embedding 请求。
    pub async fn list(&self) -> Result<MemoryList, MemoryError>;
    pub async fn recall(&self, query: &str, limit: usize, cancel: &CancellationToken) -> Result<Recall, MemoryError>;
}

#[derive(thiserror::Error)]
pub enum MemoryError {
    #[error(transparent)]
    Registry(#[from] RegistryError),          // 健康回报提交失败；召回随之失败，不退回关键词召回
    #[error("memory database: {0}")]
    Authority(String),
    #[error("memory database schema version {found} does not match this Enco ({expected}); …")]
    SchemaVersion { found: u32, expected: u32 },   // 提示与 StoreError::SchemaVersion 相同
    #[error("memory index: {0}; deleting the memory-index directory is always safe and rebuilds it")]
    Index(String),
    #[error("cancelled")]
    Cancelled,
}
```

内部状态只有两份：权威连接；`index: Arc<tokio::sync::Mutex<Option<MemoryIndex>>>`（还没有打开时为空；打开后是身份、TriviumDB 与内存中的 `nodes`），`sync` 与检索期间一直持有，用 `lock_owned()` 把守卫移进 `spawn_blocking`。写入只用权威连接，不必等待正在进行的召回。

记忆的设置是 `MemorySettings::from_document(&view.config.document, &view.config.kernel)`，按配置代际的编号缓存，`sync` 与相关性过滤共用；缓存随时可以丢弃重算，不算状态。

运行期间 TriviumDB 的操作出错时返回 `MemoryError::Index`，不做就地修复：删除 `memory-index/` 目录永远是安全的，下次启动会重建。

## 8. 常量（enco-host `limits.rs`）

```rust
pub const MEMORY_RECALL_DEFAULT: u64 = 8;
pub const MEMORY_RECALL_MAX: u64 = 20;
pub const MEMORY_RELEVANCE_THRESHOLD: f64 = 0.5;
pub const EMBED_BATCH: usize = 64;
pub const MEMORY_TEXT_BYTES: usize = 2_000;
```

## 9. 目前不做

实体与关系图、图扩散式的联想召回、DPP 去重、疲劳（这些都是 TriviumDB 已有的能力，接入时不需要换引擎）；从 Session Log 派生的对话记忆（架构文档 §3.6 的 Observer）；自动抽取与整理记忆；本地 embedding 模型。
