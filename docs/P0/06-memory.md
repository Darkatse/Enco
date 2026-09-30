# 06 记忆（enco-host `memory/`）

记忆是关于主人的长期信息：偏好、人物、约定、重要的事。它是宿主提供的能力，经内核已有的两个端口接入：写入是工具（Tool），召回是上下文源（ContextSource）。**内核不知道记忆的存在**，也不为它增加任何概念。

## 1. 结构与三条规则

```text
   memory_save / update / forget / search              召回（每个 Round，经 ContextSource）
                     │                                          │
                     ▼                                          ▼
   ┌───────────────────────── Memories（归属者）─────────────────────────┐
   │   权威：memory.db（SQLite）   ──────对账──────▶   索引：TriviumDB（派生） │
   └────────────────────────────────────────────────────────────────────────┘
                                        │ embed
                                        ▼
                      Provider 端口 → Wasm 插件 → /embeddings
```

全部机制都从下面三条规则推出。遇到规格没有写到的情况，先回到这三条。

1. **归属**：记忆跨 Session 共享，是可变状态，由唯一的归属者 `Memories` 管理（架构文档 §3.6 规则一）。它与 Scheduler 管理提醒是同一个模式：主人的 CLI 命令与 Agent 的工具调用走同一组方法。
2. **权威与派生**：`memory.db` 是权威。TriviumDB 索引是权威的函数：打开时与每次召回前，都用同一个对账函数比较两者并补齐差异，所以不需要在任何地方记住"哪些还没进索引"。对索引有任何疑问（打不开、embedding 模型或维度变了），就删掉重建，不做修补。
3. **索引提名，权威裁决**：检索命中的每一条，都回到权威读取当前内容之后才能返回；权威中已经不存在的命中直接丢弃。因此过期的索引只影响召回质量，永远不会让已经更正或遗忘的内容重新出现。

## 2. 权威：memory.db（`memory/authority.rs`）

- 文件：`$ENCO_HOME/.data/memory.db`，与 `enco.db` 分开：一个归属者，一个文件。
- 连接方式与 Store 相同（03 §3.1）：一个 `rusqlite::Connection` 放在 `std::sync::Mutex` 中，在 `spawn_blocking` 中执行；`PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;`；写事务使用 `BEGIN IMMEDIATE`。
- 版本使用 `PRAGMA user_version`：为 0 时建表并设为 1；为 1 时直接使用；大于 1 时返回 `MemoryError::NewerSchema`，拒绝启动。

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

- 目录 `$ENCO_HOME/.data/memory-index/`，其中是 `memory.tdb`（以及 TriviumDB 自己的附属文件）和 `index.json`：`{"model": "...", "dimensions": n}`，记录建索引时使用的 embedding 模型。
- 打开：`Database::<f32>::open_with_config(path, Config { dim: dimensions, load_text_index: false, ..Default::default() })`，其余保持默认。**不要**调用 `enable_auto_compaction`（它会启动后台线程）。
- 每条记忆对应一个节点：向量是记忆文本的 embedding，payload 是 `{"memory": "<MemoryId>", "rev": n}`。节点 ID 由 TriviumDB 分配，只在索引内部有意义，不出现在索引之外。
- 内存中维护 `nodes: HashMap<MemoryId, IndexedMemory>`，`IndexedMemory { node, rev }` 明确区分索引节点与权威修订号，打开时从 payload 重建。
- **替换一条记忆 = 删除旧节点 + 插入新节点 + `index_text(新节点, 文本)`。** 不在原节点上更新向量或重新 `index_text`：一种写法，语义最清楚。一批操作结束后调用一次 `build_text_index()`。
- TriviumDB 的 API 是同步的，全部调用放在 `spawn_blocking` 中。

打开流程由 TriviumDB 0.8.8 的两个已核实行为决定：WAL 只记录节点与边，崩溃后节点可以恢复；文本索引只在内存中，不写 WAL。所以每次打开都用权威中的文本重建文本索引，而不是依赖它自己的持久化。

### 3.2 打开与对账

```text
open():
    if index.json 不存在，或与配置的 (model, dimensions) 不一致:
        删除整个目录，重新创建，写入 index.json
    db = 打开 memory.tdb；失败则删除目录并重建一次；再失败返回 MemoryError::Index
    active = 权威中全部记忆的 (id, rev, text)
    r = reconcile(active 的 (id, rev), 索引中每个节点的 (NodeId, 解析出的 (MemoryId, rev) 或 None))
    删除 r.delete 中的节点；nodes = r.keep
    for 每个保留的节点: index_text(节点, 权威中的文本)
    build_text_index(); flush()
    重复 sync(None)，直到 unindexed 为空、没有进展或某一次失败
```

`flush()` 只在打开时调用：运行期间的写入留在 TriviumDB 的 WAL 中，下次打开时由它自己回放。

embedding 服务不可用时照常打开，未进入索引的记忆在召回时直接纳入（§4）。守护进程不因此拒绝启动。

### 3.3 对账（`reconcile` 与 `sync`）

`reconcile` 是纯函数，打开时与运行中共用：

```text
reconcile(active: [(MemoryId, rev)], nodes: [(NodeId, Option<(MemoryId, rev)>)]) -> { delete, keep, unindexed }
```

一个节点被保留，当且仅当它的 payload 可以解析、`active` 中有同 id 同 rev 的记忆、并且这条记忆只有这一个节点；其余节点进入 `delete`。`active` 中没有被保留节点的记忆进入 `unindexed`。打开时 `nodes` 来自扫描 payload；运行中来自内存中的 `nodes`。

运行中，写入只改权威，不触碰索引，也不留下任何标记。每次召回都先执行一次 `sync`（持有索引锁），与召回共用一次 embedding 请求：

```text
sync(query: Option<&str>, cancel) -> Result<Synced, MemoryError>
    active = 权威中全部记忆的 (id, rev)，由权威查询按 updated_at 从新到旧、id 打破同时间并列
    r = reconcile(active, nodes)
    删除 r.delete 中的节点
    batch = r.unindexed 中按 updated_at 从新到旧的前 EMBED_BATCH - 1 个
    rows = 权威中 batch 的当前内容（期间被遗忘的跳过）
    inputs = [query（如果有）] ++ rows 的文本
    if inputs 为空: return Synced { query_vector: None, unindexed: [], failure: None }
    vectors = select { provider.embed(inputs), cancel.cancelled() => return Err(Cancelled) }
    成功，且数量与维度正确:
        替换每个 row 的节点，节点的 rev 取自 row；build_text_index()
        return Synced { query_vector, unindexed: r.unindexed 去掉 rows, failure: None }
    否则（失败，或数量、维度不符，视为 provider.bad_response）:
        return Synced { query_vector: None, unindexed: r.unindexed, failure }
```

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
- 不设相关性阈值：阈值依赖具体的 embedding 模型，P0 固定取前 `limit` 条，由提示词说明它们"可能相关"。
- 没有新 Event 被消费时，查询文本不变（§5）；记忆写入与对账仍可能改变召回结果，不能据此保证前缀稳定。
- 每个 Round 贡献一次上下文（04 §6.3），也就是一次 embedding 请求。P0 不缓存查询向量（01 §2），后续根据实际延迟与用量判断是否优化。
- 取消只打断两种等待：索引锁与 embedding 响应。已经开始的 `spawn_blocking`（SQLite、TriviumDB）等它结束，然后返回 `MemoryError::Cancelled`。

## 5. 上下文源：MemoryContextSource（`memory/context.rs`）

实现 `ContextSource`（04 §2）。查询文本是 `ContextQuery.latest_event` 的规范消息文本（`canonical_message().joined_text()`），取消令牌是 `ContextQuery.cancel`；没有 Event 时只提供置顶记忆。

贡献中的候选按下面的顺序排列。顺序就是优先级：composer 按顺序纳入，直到记忆预算用完（05 §4.2）。置顶保证优先级，不保证无条件纳入；超预算的置顶记忆也记录在 `ContextPlan.omitted` 中，不因此让 Round 失败。

1. 全部置顶记忆，按 id 顺序。
2. `recall(query, MEMORY_RECALL_DEFAULT)` 的 `memories`，去掉置顶的。
3. `recall` 的 `unindexed`，去掉置顶的。

每个候选为 `Candidate { id: "memory:<MemoryId>", kind: Memory, text }`。

`lexical_only` 存在时，贡献中带一条省略：`Omission { source: "memory:semantic", reason: "embedding failed ({code}): {message}; memories were recalled by keywords only" }`。它随计划写入 Log，所以这次降级是可见的，而不是静默的。

读取权威失败时返回 `ContextError`：记忆的权威不可用，这一轮就明确失败。被取消时也返回 `ContextError`，内核按令牌状态把这一轮结算为 `Cancelled`（04 §9）。

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
/// 来自配置的 [embedding]（08 §3）。
pub struct EmbeddingSpec { pub model: String, pub dimensions: usize }

pub struct Recall { pub memories: Vec<Memory>, pub unindexed: Vec<Memory>, pub lexical_only: Option<Failure> }
pub struct MemoryList { pub memories: Vec<Memory>, pub unindexed: Vec<MemoryId> }

impl Memories {
    /// 打开权威与索引并完成对账（§3.2）。
    pub async fn open(paths: MemoryPaths, embedding: EmbeddingSpec, provider: Arc<dyn Provider>, clock: Arc<dyn Clock>) -> Result<Arc<Memories>, MemoryError>;
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
    #[error("memory database: {0}")]
    Authority(String),
    #[error("memory database was created by a newer Enco (schema version {0})")]
    NewerSchema(u32),
    #[error("memory index: {0}; deleting the memory-index directory is always safe and rebuilds it")]
    Index(String),
    #[error("cancelled")]
    Cancelled,
}
```

内部状态只有两份：权威连接；`index: Arc<tokio::sync::Mutex<MemoryIndex>>`（TriviumDB 与内存中的 `nodes`），`sync` 与检索期间一直持有，用 `lock_owned()` 把守卫移进 `spawn_blocking`。写入只用权威连接，不必等待正在进行的召回。

运行期间 TriviumDB 的操作出错时返回 `MemoryError::Index`，不做就地修复：删除 `memory-index/` 目录永远是安全的，下次启动会重建。

## 8. 常量（enco-host `limits.rs`）

```rust
pub const MEMORY_RECALL_DEFAULT: u64 = 8;
pub const MEMORY_RECALL_MAX: u64 = 20;
pub const EMBED_BATCH: usize = 64;
pub const MEMORY_TEXT_BYTES: usize = 2_000;
```

## 9. P0 不做

实体与关系图、图扩散式的联想召回、DPP 去重、疲劳（这些都是 TriviumDB 已有的能力，接入时不需要换引擎）；从 Session Log 派生的对话记忆（架构文档 §3.6 的 Observer）；自动抽取与整理记忆；本地 embedding 模型；相关性阈值。
