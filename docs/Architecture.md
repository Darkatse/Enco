# Enco 架构

Enco 是一个私人 Agent 助理：以 Rust 为内核，以 Wasm Component（WIT）为插件边界，可以在主人的多台设备之间漫游，并且能够修改、验证和回退自己的能力。本文描述它的架构：要解决的问题、核心概念、插件系统、多节点、上下文组装，以及按验收标准划分的路线图。

文中的"主人"指 Enco 的使用者与所有者；标注"实测"的数据来自原型测量，其余为设计判断。

---

## 0. 结论速览

1. **两个正交维度，一个地基。** 自我修改让能力随时间演进，多节点漫游让执行在设备之间移动。两者都依赖"模型可见 ⟺ 已记录"：Session 可以只凭 Log 重建，所以能换代码继续、换机器继续、崩溃后继续。
2. **Round 边界是唯一的安全点。** 新代际在这里变得可见，handoff 在这里提交，执行权在这里检查，Brief 在这里生成，崩溃后也从这里恢复。
3. **双向不泄露。** 插件只能看到 WIT，这由 Wasm 物理保证；内核与宿主不按插件或服务的身份写业务分支，这由依赖方向与公开契约在结构上保证；插件之间只能导入对方导出的接口，由内核经 `invoke` 转发。这直接针对插件系统中最常见的问题：系统与插件互相泄露。
4. **Rust 内核 + Wasm Component（WIT）插件。** 实测插件增量构建约 1s，加载约 27ms，实例化约 40µs。
5. **插件只响应调用。** 连接、轮询、定时器由宿主持有，资源由插件声明、宿主对账。替换渠道插件时在事件边界切换回调；新代码接续不了协议状态时，按协议重连并补收。承诺可恢复，不承诺零断线。
6. **默认每次逻辑调用一个新 Store。** Store 的作用域覆盖流式结果与最终结算；`Arc` 只负责代码回收。实测 trap 会让整个 Store 不可再进入。
7. **两条统一规则**（§3.6）。一、每份可变状态都有唯一的归属者，由它按顺序处理变更，插件代码只在归属者的边界替换（即 Erlang 的 gen_server）。二、回调把写入作为返回值交出，归属者把这些写入与接纳事实一起原子提交。丢失更新、重投重复、游标倒退、并发部署互相覆盖、迟到的健康结果，都由这两条消解，不需要逐条打补丁。
8. **恢复**：出厂 → 健康 → 试用，健康只统计可以归因于代际本身的失败；**安全模式**是 Agent 自愈的唯一起点；机械恢复不依赖模型；同伴节点互为 supervisor；**宿主切换必须经主人确认。**
9. **一致性选择 CP。** 可变的东西很小，交给共识（控制平面）；大的东西不可变或者只有单一写者，异步复制即可。Session 采用粘滞所有权，只能由持有者主动交出，或经主人确认强制接管，因此不会自动出现两个执行者。渠道和定时任务使用租约，到期后自动接管，任何时刻只有一个节点持有它们。
10. **跨节点只有两个原语**：`invoke`（移动调用）和 `handoff`（移动执行权）。委派是两者的组合。
11. **两种同步是两个轴**：复制决定节点存了什么，Brief 决定模型看到什么。
12. **移动端由漫游解决。** 手机在前台时是完整节点，进入后台前主动 handoff；离线时可以继续自己持有的会话、新建会话，但不能接管别处的会话，这是 CP 的代价。
13. **单机阶段就遵守"可分布"不变式**（§6），这几乎没有成本。
14. **认知负担与上下文组装：正确性在内核，策略在 composer，线上格式在 Provider。** 内核只负责记录实际请求、绑定代际、校验计划和安全模式；插槽、排序、披露与缓存优化都属于每个 Session 绑定的 composer，缓存的线上机制属于 Provider。只维护一份文档模型，服务两种读者（运行中的模型、开发中的 Agent），分三个层级。WIT 自动生成 CONTRACT.md，文档过期即构建失败（§7）。
15. **插件管理只有五条规则**（§4.10）：插件名就是目录名；导入接到导出，由主人配置接线；导入都接上的代际才进入快照；工具随代际自由变化，接口遵循 WIT semver；采纳外来代码时，扩权须经主人确认。不需要版本区间、求解器、加载顺序、优先级或运行时覆盖。多作者生态在 P4 之后开放。

---

## 1. 回到原始问题

### 1.1 两个目标

- **能力演进**：Agent 的能力集合应当以 Agent 发现需求的速度演进，每次尝试的失败都应当是便宜、可恢复的。
- **连续性**：对话、任务和上下文不被困在某一台设备或某一次进程中。

两者都是同一个底层问题的不同形式：**一项正在进行的 Agent 工作，如何在它脚下的东西（代码、机器、进程）变化时安全地继续？** 因此它们应当共用同一套机制。

### 1.2 唯一不可妥协的不变式

> **Agent 经正式更新路径提交的修改或迁移，都不能破坏 Agent（和主人）修复它的能力。**

范围必须写清楚：Agent 拥有完整的系统权限，可以用 shell 删除任何文件；不设沙盒，就没有任何架构能阻止这一点。因此这条不变式只覆盖正式更新路径（`plugin_deploy`、配置提交、handoff）。正式路径之外，由不依赖模型的原生管理入口兜底（§4.6）。

### 1.3 支撑一切的不变式

> **模型可见 ⟺ 已记录。** 任何进入模型请求的内容都先记录在 Log 中。请求只有一个构造者，即本 Session 所绑定代际的 composer：它在组装时读取当前状态（Log、Context 贡献、能力快照、时间），发出前把结果冻结为 ContextPlan 并按内容寻址记录。此后这份记录连同它引用的 Log 条目就是完整的请求，重试、查看与审计都只解析它，不重新组装。

### 1.4 "Agent 优先"的可操作定义

可发现、局部性、可验证、可逆、不绕路（正式接口足够表达真实需求）。

### 1.5 已确认的决策

| 决策 | 设计上的后果 |
|---|---|
| **此前使用的 Agent 系统的主要问题**：系统与插件互相泄露、更新不及时、不够 Agent 友好；主人倾向自己构建 | §4.1 双向不泄露，由 Wasm 的物理边界与依赖方向、公开契约共同保证。Provider/渠道适配器由 Agent 自行维护，上游 API 变化不再需要等待别人。内核、协议、SDK 和插件自建；正确性极难且已经商品化的地基（wasmtime、tokio、SQLite、openraft、rustls）使用成熟库 |
| **定位是私人 Agent bot，Space 永远只属于主人** | 单一所有者的信任模型：完整节点默认把全部能力导出给 Space；密钥按节点公钥加密后在 Space 内复制；不设计多方信任 |
| **一致性偏 CP，渠道自动接管** | §5.5：控制平面使用共识；Session 粘滞所有权；渠道和定时任务使用租约自动接管；少数派一侧的行为有明确规定 |
| **宿主切换必须经主人确认，自主权限以后再显式开通，当前暂缓** | §4.6 与 §6 的主人确认（Approval）机制；宿主自更新排在路线图后期 |
| **个人化的长期记忆与召回是私人助理的核心能力** | §6 记忆：权威是宿主 SQLite 中的记忆记录，TriviumDB（嵌入式向量 + 文本 + 图引擎）只做可重建的派生索引，embedding 经 Provider 插件调用；内核不知道记忆，记忆经 Tool 与 Context 两个端口接入 |
| **P4 之后开放多作者插件生态**；上游更新采用 Chrome 式确认；全局命名基于 git（URL 或 owner），不绑定某个托管平台 | §4.10：插件名是主人仓库中的目录名，来源是它的一项属性；第三方代码的权限就是它的导入，采纳时扩权须经主人确认。Space 仍只属于主人，插件作者不是 Space 的成员 |

关于"喜欢自己构建"：路线图的每个阶段都有验收标准，它们的另一个作用是防止构建超出需要的东西。

---

## 2. 设计透镜：Erlang、Plan 9、Smalltalk、Lisp

| 透镜 | 它把什么做到了极致 | Enco 拿走什么 | 它的教训 → Enco 的对策 |
|---|---|---|---|
| **Erlang** | 隔离进程、邮箱、位置透明、supervision、热代码加载 | 归属者 actor 持有状态，插件代际作为可替换的回调模块（gen_server，§3.6）；Binding 作为注册表；健康门控作为 supervisor | 位置透明掩盖了部分失败，netsplit 后需要人工收拾，最多两个代码版本 → 远程调用显式返回 `unknown`；控制平面使用共识；代际数量不限，由引用计数回收 |
| **Plan 9** | 按进程组合的命名空间；`import`；`cpu` 命令 | 每个 Round 的能力快照是一个命名空间，由本地能力和远程导出的能力组成；handoff 就是 `cpu` | 把一切都做成文件会丢失动作的语义 → 动作使用类型化调用，文件投影只用于检查 |
| **Smalltalk** | 活的 image，系统在运行中自我修改 | 自我修改闭环；`status`/`inspect` | image 不可复现、会腐烂；包之间的方法覆盖让结果取决于加载顺序 → git + 内容寻址制品 + 系统代际（§4.7）；没有运行时覆盖，要改变插件的行为就改它的源码并部署新代际（§4.10） |
| **Lisp** | 代码即数据，eval/REPL，语言可以扩展自身 | Log 是数据，请求是 composer 对当前输入的纯函数、并作为数据记录下来，工具调用是被内核 eval 的程序 | 无限扩展导致方言碎片化 → 5 种类型化贡献 + 少量 WIT 契约 |

Lisp 透镜还带来一个分布式要求：已记录的请求在任何节点上都解析为同一份内容，所以换机器继续时不需要重现上一次组装。composer 仍是确定性的纯函数（同样的输入得到同样的输出，由契约测试检验），它的代际写入 Attempt 记录。

---

## 3. 架构主干

### 3.1 一句话脉络

> **一切发生的事都作为 Event 送到它的归属者，需要 Agent 处理的送进 Session 当前所在节点的 Inbox；内核以 Round 为单位执行"组装上下文 → 调用模型 → 执行工具"，每个 Round 钉住一份能力快照；事实写入 Log 并复制到 Space；插件只贡献能力，节点只提供位置，两者都不拥有事实。**

### 3.2 十个概念

| 概念 | 定义 |
|---|---|
| **Event** | 送给某个归属者（Session、连接、Observer）的一件事，带全局唯一 ID，可去重。心跳、重连、健康回报、索引进度不进入对话 |
| **Session** | 身份 + Log + 工作区 + 配置（所用的 composer、Provider 与声明的要求）；单写者（一个 actor） |
| **Round** | 一次模型请求 + 其全部工具调用及结果确认；唯一的安全点。一个 Event 唤醒的连续多个 Round 称为一次 Run |
| **Log** | 追加式事实，键为 `(epoch, seq)`；模型上下文的唯一来源 |
| **Capability** | Tool / Provider / Channel / Context / Observer，ID 为 `(节点, 插件名, 名称)` |
| **Plugin / Generation** | 能力的打包单位，名字就是主人仓库中的目录名（§4.10） / 一次激活 = `(单调编号, 制品哈希, 配置引用)`；制品是内容寻址的不可变文件（§4.5） |
| **Host** | 节点提供的机制：fs、exec、http、连接、定时、状态、blob、检索索引 |
| **Node** | 一个运行中的 Enco 内核及其 Host；完整节点、Edge 节点，或见证者（只参与控制平面） |
| **Space** | 主人全部节点的集合；其控制平面（成员、Binding、期望状态、git 引用）由共识维护 |
| **Binding** | `key → (node, epoch)`，单写者资源的所有权。两种形态：**粘滞所有权**（Session）和**租约**（渠道连接、定时任务） |

### 3.3 五条路由规则

```text
1. Event          → 它的归属者；Session 的输入 → Binding(session).node 的 Inbox（不可达时进入本地 outbox，重连后投递）
2. Round 开始      ⇐ 仅当 Binding(session) 指向本节点，且 epoch 为最新
3. Capability 调用 → 能力所在节点（本地直接调用，远程 invoke）
4. 出站消息        → 持有 Binding(channel) 租约的节点
5. 事实            → 写入本地 Log，按 epoch 栅栏异步复制到 Space
```

单节点是所有 Binding 都指向自己、控制平面只有一个投票者的特例。**单机版是分布式版的退化形式，不是另一套代码。**

### 3.4 主流程

```text
 手机(learner)     桌面(learner)     VPS(投票者)          见证者(投票者)   ESP32(Edge)
 ┌────────┐       ┌────────┐       ┌──────────────┐     ┌──────────┐    ┌────────┐
 │ Kernel │◀─────▶│ Kernel │◀─────▶│ Kernel       │◀───▶│ 控制平面  │    │ Host   │
 │ Host   │       │ Host   │       │ Host         │     │ 仅此而已  │    │ only   │
 └────────┘       └────────┘       │ Telegram 租约 │     └──────────┘    └────────┘
                                   └──────────────┘
 控制平面（Raft）：成员 · Binding/租约 · 期望状态 · git 引用        ← 小、可变、线性一致
 数据平面（异步）：Session Log（epoch 栅栏）· blob · 制品 · git 对象  ← 大、不可变或单写者
 实时传输：invoke / handoff / 流订阅

Telegram 消息 → VPS 渠道插件 → Event → Binding(session)=桌面 → 桌面执行 Round
  → invoke(手机, camera_snap) → 结果写入 Log → deliver 路由到 Telegram 租约持有者(VPS)
```

### 3.5 Round 边界做什么

| 维度 | 在 Round 边界发生的事 |
|---|---|
| 代码 | 读取最新能力快照，新代际开始可见，目录变化写入 Log |
| 位置 | 提交 handoff，下一 Round 在新节点执行 |
| 权威 | 检查 Binding 的 epoch，已被取代则立即停止 |
| 上下文 | 生成压缩结果或 Brief |
| 恢复 | 从最后一个完整 Round 重建，未确认的副作用标记为 `unknown` |

### 3.6 归属与转移：两条统一规则

可变状态有几类典型的失败：共享 KV 丢失更新、写入之后 trap 再重投导致重复、同一连接跨代际时游标倒退、并发部署互相覆盖、迟到的健康结果回退了更新的版本。这里不逐条打补丁，而是把系统里已经在用的两条规则推广到所有可变状态上。

**规则一：归属。每一份可变状态都有唯一的归属者，由它按顺序处理所有变更。插件代码只是归属者调用的回调，只在归属者的边界上替换。**

| 状态 | 归属者 | 替换代码的边界 |
|---|---|---|
| Session 历史 | Session actor | Round |
| 渠道连接或账号的协议状态与游标 | 连接 actor（每个连接或账号一个） | 事件 |
| 插件的持久状态 | 状态作用域 actor（插件声明作用域：全局 / 每 Session / 每账号） | 调用 |
| 记忆（及其索引） | 记忆的归属者（宿主）；索引是它的派生物 | 调用 |
| 由 Log 派生的索引（例如对话索引） | Observer actor，持有 Log 游标 | 事件 |
| 能力注册表（代际、健康、接线） | 部署提交者（唯一） | 提交 |
| Binding、租约、期望状态 | 控制平面（多节点时为共识；单节点时就是本机 SQLite） | 提交 |

这就是 Erlang 的 gen_server：进程拥有状态和顺序，回调模块可以热替换。actor 跟着状态走，而不是跟着代际走。它也回答了"事件去哪里"：每个 Event 送到它的归属者，只有需要 Agent 处理的才进入 Session Inbox。

**派生状态是其权威的函数**，可以随时丢弃并从权威重建，因此它的归属者就是维护它的那一方，不需要另立规则：由追加式 Log 派生的，用游标增量推进（Observer）；由可变记录派生的（例如记忆索引），在使用前与权威按差异对账。

无状态的工具没有归属者：每次调用一个新 Store，完全并发。

**规则二：转移。回调把写入作为返回值交出；归属者检查前置条件后，把这些写入与"接纳事实"放在同一个事务里提交。**

- **渠道事件**：`(连接状态, 事件) → {状态写入, 入站消息, 待发送帧}`，一次提交。游标不可能先于消息被接纳而推进；trap 意味着什么都没有提交，所以重投是安全的；入站消息按 `(来源, 账号, 事件 ID)` 去重。
- **有状态的工具调用**：回调读取已提交的快照，返回 `outcome` 和状态写入，二者与工具结果一起提交。同一作用域内的调用由归属者串行执行，所以不会丢失更新，插件 API 里也不需要 CAS。
- **Observer**：派生写入与游标一起提交，可以从任意已提交位置幂等重放。
- **注册表**：部署、晋升、回退都携带"期望的当前代际"；不匹配时只记入历史，不改动路由。两个并发部署、迟到的健康结果都由此自然处理。提交前按 §4.10 检查接线与准入。
- 反应期间允许只读或幂等的操作（HTTP 读取、写入内容寻址的 blob）；有副作用的写入只能作为返回值。已经发出的外部效果（HTTP 写、exec）按 `outcome` 规则结算：调用中途 trap 或超时，而宿主记录显示效果已经发出，结果就是 `unknown`。trap 不会撤销已经发生的外部效果，所以重投以记录为准，不以"trap 了"为准。

两条规则之下，只剩两种调用形态：

- **调用**：工具、Provider、deliver。可能有外部效果，返回 `ok / failed / unknown`。
- **转移**：渠道事件、Observer。返回写入，由归属者提交。

普通工具、有状态工具、流式模型调用、渠道、派生索引和破坏性迁移，寿命各不相同，但都是这两种形态与上面归属表的组合，不需要为每一类单独建一套框架。这两条规则也不是新发明：Session actor（规则一）和"模型可见 ⟺ 已记录"（规则二的一个特例）一开始就在系统里，这里只是让它们覆盖所有可变状态。

---

## 4. 插件系统

### 4.1 边界：双向不泄露

系统与插件互相泄露是插件系统最常见的问题。泄露有三个方向，每个方向都要有机制保证，而不能只靠约定：

| 方向 | 规则 | 保证手段 |
|---|---|---|
| 插件 → 宿主 | 插件只能调用 linker 提供的 WIT 导入；没有共享内存、没有宿主对象、不能 monkey-patch | **Wasm 的物理隔离**：组件只能访问 linker 中存在的导入 |
| 宿主 → 插件 | 内核与宿主不按插件或服务的身份写业务分支。服务之间的差异（例如渠道是否支持编辑、线程、打字状态）只能通过插件声明的能力表达。宿主按操作系统平台做适配是它的正常职责 | **结构保证**：kernel/host 不依赖任何插件 crate；插件身份在内核中只是不透明的 ID，没有插件枚举；唯一的契约是 WIT（lint、CONTRACT.md、破坏性变化检查），kernel/SDK 的公开 API 以 API.md 快照受 `--check` 约束（§7.4）。**行为验证**：P2 验收要求 Agent 只读手册与插件源码完成修复 |
| 插件 ↔ 插件 | 只能导入对方导出的 WIT 接口，由宿主经内核 `invoke` 转发；插件不引用其他插件的名字；KV 按插件隔离；不存在全局共享状态 | 插件只看得到自己导入的接口，接到哪个实现由主人配置决定（§4.10）；WIT 中没有能访问其他插件状态的导入 |

原生实现的例外必须登记在 `docs/decisions/native-exceptions.md`，写明缺失的宿主能力和迁移条件。

不扫描源码中的名字：那是对源码文本的断言，与测试准入相冲突；它会催生白名单，也无法证明边界正确（按身份分支的代码未必出现名字，出现名字的代码也未必越界）。"按身份分支"属于评审问题，由 `maintain-conceptual-integrity` 的检查清单承担。

术语：本文中"平台"只指操作系统或运行环境（Linux、Android、iOS），Telegram、QQ 等称为"渠道"或"服务"。一个词只表达一种含义。

效果：宿主升级不会破坏插件，因为契约只有 WIT；插件更新也不需要修改宿主。WIT 演进采用 Zed 的做法（宿主同时链接历史版本的 world 并做适配）；个人项目早期可以简化为"WIT 大版本变化时，由宿主触发全部插件重建"，Agent 自动完成；这项简化只在插件源码全部归主人时成立，开放插件生态（§4.10）之前必须改为同时链接历史版本。这就回应了"更新不及时"：没有任何一层需要等待另一层。

### 4.2 四层扩展机制

| 层 | 形式 | 何时使用 | 修改成本 |
|---|---|---|---|
| Skill | `SKILL.md` + 脚本 | 流程知识、胶水、一次性自动化 | 零构建 |
| MCP | 外部进程 | 复用现成生态 | 重启进程 |
| Wasm 插件 | Rust 组件（WIT） | 类型化、长期维护、常驻、可热替换、可跨节点 | 增量构建约 1s |
| 原生 crate | 编进宿主 | 内核、Host、CPU 密集计算、平台桥接、管理通道 | 宿主重建 + 主人确认 |

判据：能用 Skill 就不写插件，能写插件就不改宿主。

### 4.3 插件只响应调用

宿主持有 WebSocket、长轮询、定时器、Webhook 路由和插件状态；插件导出反应函数，按 §3.6 返回转移。

- **资源是声明式的。** `describe(config)` 返回插件需要的资源（连接、定时、Webhook 路由，每项有稳定的名称，宿主按插件名划分它们的命名空间），宿主负责对账：没变的保留，变了的重建，删掉的撤销。这与 §4.7 的"期望状态 / 实际状态"是同一个模式。用户创建的提醒这类持久任务是内核事实，通过 `schedule_create` 能力创建，不属于插件资源。
- **替换渠道插件时，默认在事件边界切换回调。** 如果新代码接续不了当前的协议状态（在 `describe` 中声明 `reconnect-on-upgrade`，或者连接选项发生了变化），就按协议重连并补收：Telegram 用 offset，Discord 用 RESUME。连接级的传输状态（例如流式解压）属于宿主传输层的连接选项，不是插件状态。**承诺的是可恢复，不是每次替换都零断线。**
- **渠道故障转移**：由新的租约持有者重建连接，从已复制的连接状态继续。

### 4.4 契约（WIT 草图，P1 定稿）

```wit
package enco:plugin@0.1.0;

interface types {
  type json = string;
  record failure { code: string, message: string, retryable: bool }
  variant outcome { ok(json), failed(failure), unknown(failure) }   // unknown = 可能已执行
  record blob-ref { hash: string, mime: string, size: u64 }
  record call-context { session: string, round: string, call-id: string, node: string }
  record state-write { key: string, value: option<list<u8>> }        // none = 删除
  /// 转移：回调交给归属者的全部写入，一次提交（§3.6）
  record transition { writes: list<state-write>, inbound: list<inbound-event>, send: list<frame> }
  record call-result { outcome: outcome, writes: list<state-write> }
}

/// 只包含读取与"效果会被记录"的操作；状态写入、入站事件、发送帧只能作为返回值
interface host {
  use types.{blob-ref};
  log: func(level: log-level, message: string);
  state-get: func(key: string) -> option<list<u8>>;       // 读已提交的快照
  blob-put: async func(mime: string, data: stream<u8>) -> blob-ref;   // 内容寻址，幂等
  blob-read: async func(r: blob-ref) -> stream<u8>;
  exec: async func(req: exec-request) -> result<exec-output, string>;
  http: async func(req: http-request) -> result<http-response, http-failure>;   // 带 request-sent
}

interface tools {
  use types.{json, call-context, call-result};
  enum effect { read-only, idempotent, side-effect }
  record tool-spec { name: string, description: string, input-schema: json, effect: effect }
  list-tools: func() -> list<tool-spec>;
  call: async func(ctx: call-context, name: string, input: json) -> call-result;
}

interface channel {
  use types.{json, transition, call-result};
  on-event: async func(ev: host-event) -> result<transition, failure>;      // 帧 / 定时 / 连接状态
  on-webhook: async func(req: http-request) -> tuple<http-response, transition>;
  deliver: async func(target: string, message: json) -> call-result;
}

interface observer {
  use types.{state-write};
  on-log: async func(events: list<log-event>) -> list<state-write>;        // 游标由宿主持有
}

interface provider {
  use types.{failure};
  /// 消息与工具调用是协议本身，用 WIT 类型表达；P0 的完整定义见 wit/plugin.wit 与生成的 wit/CONTRACT.md
  record request { messages: list<message>, tools: list<tool-spec>, max-output-tokens: option<u32> }
  complete: async func(settings: settings, request: request) -> result<completion, failure>;   // 非流式
  embed: async func(settings: settings, inputs: list<string>) -> result<list<list<f32>>, failure>;   // 供宿主的检索索引使用
  // 流式以新增函数加入：stream: async func(settings: settings, request: request)
  //   -> tuple<stream<completion-delta>, future<result<completion, failure>>>;
}

interface lifecycle {
  describe: func(config: json) -> plugin-info;   // 贡献、所需资源、状态作用域、reconnect-on-upgrade；不含插件名（§4.10）
  probe: async func() -> result<_, string>;      // 只依赖宿主持有的 fixture，不依赖外部服务
}

world base { import host; export lifecycle; }
// world telegram { include enco:plugin/base; export enco:plugin/channel; export enco:plugin/tools; }
// world summarizer { include enco:plugin/base; import alice:web/fetch@1.0.0; export enco:plugin/tools; }
//   插件之间没有按名字调用的导入：要使用其他插件的能力，就导入它导出的接口，由宿主转发（§4.10）
```

组件的 imports 就是它的全部需求：Host 接口由本机满足，其他接口接到已准入插件的导出（§4.10）。imports 可以从制品中读出，因此插件能否在某个节点上运行，是部署前就能回答的问题。

宿主不提供状态写入、事件投递、建立连接或注册定时这类命令式导入：它们都改为返回值或声明，契约因此更小。

### 4.5 制品、代际、调用与热替换

三种寿命不同的东西分别命名：

| 名称 | 是什么 | 寿命 |
|---|---|---|
| **制品**（Artifact） | 不可变的组件文件，按内容哈希寻址，复用 `InstancePre` | 永久（可垃圾回收） |
| **代际**（Generation） | 一次激活 = `(单调编号 n, 制品哈希, 配置引用)`。健康状态属于代际；同一份制品配上不同配置是不同的代际 | 从激活到被取代 |
| **调用**（Invocation） | 一次逻辑调用，独占一个 Store；作用域覆盖导出函数、它返回的全部 stream/future，以及最终结算 | 到结算为止（不是到导出函数返回为止） |

- **发布**：构建与校验可以并发进行，提交则只经过唯一的部署提交者，顺序是：制品完整写入 → 兼容性检查 → 在 SQLite 事务中提交代际记录 → 发布内存快照（`ArcSwap`）。如果在最后两步之间崩溃，重启后按数据库重建快照。Round 开始时执行 `load_full()`。
- **回收**：`Arc` 只负责代码的回收。在途工作由调用记录（它本来就在 Log 里）说明。
- **失败的代际**：如果某个 Round 钉住的代际被标记为失败，这个 Round 里对它的后续调用会明确返回 `failed(generation_revoked)`，要到下一个 Round 才看到回退后的目录。不会在同一个 Round 里悄悄换掉实现。
- **破坏性状态迁移**：由归属者在替换边界执行一次（相当于 Erlang 的 `code_change`），该插件因此放弃自动回退。其他情况只做 expand；SDK 的状态类型默认保留未知字段，这样旧代码回写时不会丢掉新字段。
- **取消** = 发出信号 + 等待静止：先 abort，再等待真正结束。Wasm 调用另外设置外部超时和 epoch 中断。

### 4.6 健康门控、恢复层级与宿主切换

```text
出厂代际（嵌入宿主二进制） ← 最近健康代际 ← 试用代际（probe + 前 N 次调用通过才晋升）
```

- 试用代际失败时自动回退，并投递 `deploy.rolled_back` Event。晋升和回退都携带期望的当前代际（§3.6），迟到的结果不能改动更新之后的路由。回退可能使依赖其导出的插件退出快照，原因写入 Log（§4.10）。
- **健康只统计能归因于代际本身的失败**：trap、契约违规、宿主持有的确定性 fixture 失败。网络中断、限流、凭证失效等外部失败由宿主分类，不会触发回退，因为它们在所有版本上都一样。"前 N 次没有 trap"只说明运行层面健康，不说明语义正确；语义正确靠 fixture 和契约测试。
- **出厂代际是内置的候选，不保证一定能用**：它仍然依赖兼容的 Host 接口、有效的配置和凭证，以及外部服务。
- **Provider 与 Attempt**：一个 Round 内可以有多次模型尝试（Attempt），每次记录实际使用的代际、请求引用和结果。只有成功结算的那次 Attempt 的工具调用会被分派，失败尝试迟到的 token 一律丢弃。Provider 失败后改用健康代际，就是开始一次新的 Attempt；如果旧代际无法表达当前请求，就在明确的边界上重新组装。**工具绑定以 Round 为单位固定，模型请求与适配器以 Attempt 为单位固定。**
- **安全模式**：所有策略点（`round.compose`、`inbound.preprocess`、`tool.gate`）改用原生出厂实现，Provider 使用出厂代际，只暴露救生集，不加载普通 Skill 和 Observer，使用最小系统提示词。正常路径连续失败时自动进入，主人也可以手动进入。它是 **Agent 自愈的唯一起点**。
- **机械恢复不依赖模型**：原生管理入口（本地 CLI/HTTP）可以列出代际、查看日志、停用插件、选择旧配置、进入安全模式和重启，模型完全不可用时也能操作。它不可被插件替换。Android 上需要一个不依赖 WebView 或聊天渠道的原生入口。
- 同伴节点可以通过 `invoke(node, shell_exec, …)` 诊断彼此。
- **宿主切换必须经主人确认。** Agent 可以自主完成宿主的构建、测试和预检，然后把"切换到宿主 vN+1"作为待确认命令提交（§6 Approval）；主人确认后由 supervisor 执行，健康检查失败则自动回滚到上一个二进制。自主切换的权限以后通过显式授权开通。宿主自更新整体排在路线图后期。
- **插件的热替换不需要确认**：它发生在健康门控与自动回退的保护下，恢复层级不依赖被替换的东西本身。

### 4.7 期望状态与系统代际

- **期望状态**放在控制平面，线性一致：`插件名 → generation hash`、外来插件已批准的导入集合（§4.10），以及配置和 skills 的 git 引用。在任何节点上修好一个插件，其他节点都会收到。
- **实际状态**按节点记录：只激活通过准入（§4.10）的代际，健康状态也按节点记录。
- **制品**按哈希向同伴拉取（类似 Nix binary cache）。
- **系统代际** = `(宿主版本, 活跃插件代际, 配置提交, skills 提交)`，可以命名、比较和整体回滚。新设备加入时采用期望状态，就得到同一套能力。

### 4.8 自我修改闭环

```text
~/.enco/                      ← git 仓库
  AGENTS.md  config.toml  skills/  plugins/<name>/
  .enco/store/<hash>.wasm     ← 制品库（内容寻址）
  .enco/data.sqlite           ← Log、KV、本地的控制平面副本
  .enco/memory.sqlite         ← 记忆（权威）
  .enco/memory-index/         ← 记忆索引（派生，可删除）
```

- 命令（CLI 与 Agent Tool 同源，Tool 名为 `plugin_<动作>`）：`scaffold / build / test / deploy / status / rollback / logs`；外来插件另有 `install / update`（§4.10）。
- `plugin_build` 是只由装有工具链的节点导出的能力，手机上的 Agent 通过 `invoke(vps, plugin_build, …)` 构建，再按哈希取回制品。
- 构建诊断是结构化的，只返回前 N 条，完整日志存为 blob。构建以插件目录的内容快照为输入（构建前自动提交该目录，避免未提交的修改与产物对不上），每个代际记录 `{plugin, 源码快照, Cargo.lock 摘要, artifact_hash, wit_version, toolchain, config_ref}`。
- 构建流程同时生成该代际的运行时手册，并更新插件 README 中的生成区（§7.5、§7.6）。
- git 仓库在节点之间同步对象，`main` 的推进是控制平面上的 CAS（§5.5）。

### 4.9 状态与迁移

插件状态只放在宿主 KV 中，由其归属者串行写入（§3.6），并随 Space 复制；实例内存只作缓存。迁移遵循 expand-contract，并且必须兼容 Space 中较旧的节点（破坏性迁移见 §4.5）。**数据格式兼容、并发访问正确、业务语义兼容是三个独立的条件**：归属者保证第二条，expand 规则保证第一条，第三条靠契约测试。用户创建的持久任务是内核事实，不属于任何代际。

### 4.10 插件的命名、接线与生态

插件系统的依赖地狱通常来自四个条件：
- 共享的全局命名空间：JVM classpath、Garry's Mod 的 `_G`、Smalltalk 的 SystemDictionary。
- 通过修改别人来扩展：Minecraft 的 Mixin、Pharo 的方法覆盖、hook 返回值截断其他 hook。修改的先后会影响结果，于是需要加载顺序和优先级。
- 同名多版本加区间约束：可安装性因此是 NP 完全问题，Fabric 为此引入了 SAT 求解器。
- 没有人为一组插件的组合负责：整合包、Quicklisp dist、OTP release 都是事后的补救。

前三个在 Enco 中由结构消除：组件之间不共享任何东西，各自静态链接依赖；插件无法修改别人，也没有启动顺序（§4.3）；每个节点上每个插件只有一个活跃代际。第四个由系统代际承担（§4.7）。

剩下的是命名、接线、准入、演进和信任，各由一条规则处理，不引入新概念：名字是插件的属性，接线与准入是注册表提交时的检查（§3.6 规则二），演进交给 WIT，信任复用主人确认。

**1. 名字：每个插件只有一个本地名字。**

- 插件名就是主人仓库 `plugins/` 下的目录名，使用 kebab-case（与 WIT 标识符同一规则）。目录不会重名，仓库又在 Space 内共享，所以插件名在 Space 内的唯一性由结构保证。
  - 安装时默认采用作者建议的名字，撞名即失败，由主人另选。
  - 改名就是 `git mv`，作为一次目录变化写入 Log。
  - 原生能力与出厂插件的名字由宿主保留，例如 `fs`、`shell`、`memory`、`schedule`。
- 插件不自报身份。扩展字段由内核在记录 Attempt 时标注所用的插件，只回放给同名插件。
- 能力 ID 为 `(节点, 插件名, 名称)`，显示为 `web_search@vps`。
  - 模型可见的工具名是 `插件名_名称`：名称使用 snake_case，第一个下划线就是分隔符，总长受服务商上限约束（OpenAI 与 Anthropic 均为 64 个字符，由常量生成）。
  - 名字只由插件自身决定，不随其他插件的安装而变化。composer 决定披露哪些工具、披露到哪一级，但不决定名字。
- 契约名是 WIT 包名 `owner:package`。WIT 标识符不能包含 `.` 与 `/`，所以契约名使用 owner 而不是 URL。
- 外来插件的来源（git URL + 已采纳的 commit）是目录的一项属性，随源码记录在 git 中，不绑定 GitHub、GitLab 或 Gitee 中的任何一个。仓库迁移时只改来源，插件名不变。

编进制品的名字（工具名、契约名）全局有意义；主人分配的名字（插件名）只在本 Space 内有效；插件从不引用另一个插件的名字。因此不会出现 crates.io、PyPI 那种"撞名只能改名"的局面：本地撞名只需换一个目录名，作者的代码不需要改动。

**2. 接线：依赖、槽位和贡献是同一件事。** 插件导出接口、导入接口，主人经配置把导入接到导出上。依赖、槽位和贡献的区别只在导入方要一个还是要全部，这由导入方决定。

- 插件的每个导入接到 Host，或恰好一个导出。候选唯一时自动接上；不唯一时由主人配置选择，否则准入失败，并列出候选。
- 内核也是导入方：

| 内核导入 | 数量 | 选择写在哪里 |
|---|---|---|
| composer、Provider | 一个 | Session 配置 |
| 策略点（`inbound.preprocess`、`tool.gate`） | 一个 | 节点配置 |
| 工具、Context 贡献、Observer、渠道 | 全部 | — |

- 顺序由消费者决定：Context 的顺序归 composer，其余"全部"类的导入彼此可交换。插件不声明优先级或先后，也不能覆盖其他插件；要改变另一个插件的行为，就修改它的源码并部署新代际。
- 接线时做类型检查。同一个契约有多个实现是正常的；名字相同而类型不同的契约会被类型检查拦下，不需要另立规则。
- 跨插件调用由宿主转发，内部走内核 `invoke`：被调用方每次调用使用一个新 Store，照常结算 `outcome`，也可以跨节点。
  - 插件之间的接口只使用值类型。
  - 每个函数返回 `result<T, call-error>`，其中 `call-error` 由内核定义，包含 `unknown` 与 `unavailable`。
  - 这两条都由契约 lint 检查（§7.3）。
- 模型调用工具，代码导入接口。插件不按名字调用其他插件的工具。

**3. 准入：导入都接上的代际才进入快照。**
- 接到 Host 或已准入插件的导出都算接上。不满足的代际连同原因写入 Log，对模型可见。
- 循环依赖不需要特殊处理，因为插件是被动的，没有启动顺序。
- 部署是有意的命令，不能拆掉正在使用的接线；被拒绝时，错误信息指出使用者和下一步。
- 回退是机械恢复，可以拆掉接线；受影响的插件退出快照并记录原因，因为恢复不能依赖被替换的东西本身。
- `plugin_status` 显示每个插件的导入、导出、使用者，以及未准入的原因。

**4. 演进：只有代码消费者需要兼容规则。**
- 工具的读者是模型。模型每一轮都读取当前的定义（记录在 ContextPlan 中），所以工具可以随代际自由变化；改名会让引用它的要求失效，由校验报出。
- 接口的读者是代码，遵循 WIT semver：同一主版本内只增不改，破坏性变化就升主版本。wasmtime 的 linker 按主版本匹配。
- Enco 没有另外的兼容规则，也没有版本区间和求解器。每个导入只接一个导出，检查就是类型检查。

**5. 信任：只在外来代码进来时检查。** 插件作者不是 Space 的成员，他们的代码在主人的授权下运行，权限就是它的导入。Wasm 保证插件只能调用它导入的接口，而导入可以从制品中读出，所以不需要手写权限清单。

- 安装与更新都是采纳外来修订。采纳后的导入如果超出上次批准的集合，采纳就进入待确认状态（§6 主人确认），确认之前仓库和运行中的代际都不变。
  - 首次安装时已批准的集合为空，所以总要确认；更新只在扩权时确认，这与 Chrome 扩展的做法相同。
  - 已批准的集合属于期望状态（§4.7）。
- 部署路径对所有插件相同，不看来源。Agent 本来就有 shell，限制它能修改哪些代码没有意义。
- 采纳总是显式操作，不在后台自动更新，采纳前由 Agent 审阅 diff。
- 模型可见的文本随代际固定，并按内容寻址；任何变化都会产生新代际，并出现在 README 生成区的 diff 中。所以 MCP 那种"批准之后悄悄改写工具定义"的 rug pull 在这里不成立。
- 导入只能限制代码能做什么，不能限制它在权限之内做什么。Skill 的脚本和 MCP 服务器在 Wasm 边界之外，没有导入可以比较，采纳它们等于完全信任。

**生态。** 多作者生态在 P4 之后开放。
- 分发的单位是源码：外来插件按 commit 引入主人仓库，在 Space 内构建，制品按内容寻址。
- 本地修改与上游更新用 git 合并。Agent 在本地修好的问题可以向上游提交 PR；这属于对外发布，需要主人确认。
- 可以导入的接口，就是 Host 接口加上已安装插件导出的接口，它们都出现在运行时手册中（§7.5）。
- 缺少导入时不自动安装，由 Agent 提议、主人确认。
- 开放生态之前必须满足三个条件：`enco:plugin` 达到 1.0 的稳定度；宿主能同时链接历史版本的 world（§4.1）；`enco-sdk` 按 semver 发布。

刻意不做的事：
- 版本区间与求解器。
- 加载顺序与启动阶段。
- 优先级数字与运行时覆盖。
- 库插件与插件继承：共享代码是 crate，在编译期静态链接。
- 多插件原子部署：expand-contract 已经够用，Space 中有旧节点时本来就必须这样做（§4.9）。

---

## 5. 多节点：Space、Binding 与漫游

### 5.1 从 Operit2 借鉴与改进

沿用：节点对等；Space 不是执行容器；Binding 只表达"下一步在哪执行"；持久复制与实时传输是两条独立路径；交接边界是"工具结果已持久化、下一轮模型请求尚未开始"；continuation 以 `(key, generation)` 保持幂等；响应流属于发起方；密钥与 Host 状态留在本地；Edge 节点只提供能力；运行时实例不迁移，只迁移任务事实。

改进（依据本地源码核对）：

| Operit2 现状 | 问题 | Enco |
|---|---|---|
| 冲突顺序 `SyncOperationOrder { createdAt, originDeviceId, sequence }`，以墙钟为首键做 LWW | 时钟偏差会让较旧的写入胜出 | 没有 LWW：可变状态进入控制平面（线性一致），大数据不可变或单写者 |
| 目标不可达时由发起设备比较写入接管 | 分区下旧节点的副本仍然指向自己，可能出现双执行者（偏 AP） | CP：Session 粘滞所有权，只有主人能打破；渠道使用由多数派授予的租约 |
| 业务数据全量复制到所有成员 | 手机的存储与流量 | 日志全量复制，blob 和制品按哈希按需拉取，支持仅本地的 Session |
| 只有 `switch_core` 迁移 | 缺少轻量的跨节点方式 | 增加 `invoke`；委派 = 新 Session + handoff |
| 插件跨节点接续尚未设计 | — | Wasm imports 检查 + 按哈希分发 + 复制的 KV |
| 覆盖网络多跳路由（`CoreNodeRouter.rs` 5405 行） | 个人场景下收益有限 | v1 由 Tailscale/WireGuard/EasyTier 等载体负责可达性 |

### 5.2 三种位置

| 位置 | 由什么决定 |
|---|---|
| 界面位置（在哪里看、在哪里说） | 任意节点都可以订阅实时流、投递 Event |
| 执行位置（下一个 Round 在哪执行） | `Binding(session)` |
| 能力位置（工具在哪里运行） | 能力 ID 上的节点限定 |

三者相互独立。在手机上说话、由 VPS 思考、用桌面的 IDE、再拍一张手机照片，这是一个 Session 的正常状态，不需要迁移。

### 5.3 两个跨节点原语

**`invoke(node, capability, input) -> outcome`**（Erlang `rpc:call` / Plan 9 `import`）

- 远程能力以节点限定的 ID 进入 Round 快照。模型可见的名字不包含节点（§4.10），否则工具列表会随节点数成倍增长。方向（P4 定稿）：只在一个节点上存在的能力，在描述中写明节点；与位置无关的能力由内核选择执行节点；只有在多个节点上都存在、且结果取决于位置的能力（例如 `fs_*`、`shell_exec`、剪贴板）才带 `node` 参数。
- 跨节点时不钉住代际，而是检测变化：调用携带 schema 哈希，不一致时返回 `failed(schema_changed)`，表示确定没有执行。
- 请求发出后连接断开，一律返回 `unknown`。

**`handoff(session, node, mode)`**（Plan 9 `cpu`）

- 只在 Round 边界提交：持有者先停止执行，再在控制平面上 CAS `Binding(session) = (target, epoch+1)`，然后经实时路径发送 `{session, epoch, required_position, mode}`。
- 目标节点等副本追到 `required_position` 后开始下一个 Round，并重新计算能力快照；目录变化写入 Log。
- continuation 以 `(session, epoch)` 保持幂等。
- 发起节点的设备能力仍然可以通过 `invoke` 使用。

**委派** = 创建子 Session（首个 Event 是 Brief）+ `Binding(child)` 设为目标节点 + 父 Session 监视子 Session。子 Session 完成或崩溃，都会作为 Event 进入父 Session 的 Inbox。

发起者可以是 Agent（`session_handoff`、`task_delegate`）、策略（手机即将进入后台、能力亲和性）或主人（UI），三者走同一个内核命令。

### 5.4 两种同步：两个轴

| 轴 | 问题 | 选项 |
|---|---|---|
| 复制（节点存了什么） | 耐久性、离线可读、能否 handoff | 全量日志复制（默认）/ 仅本地；blob 与制品按哈希按需拉取 |
| 上下文（模型看到什么） | 上下文密度、任务隔离 | 完整历史（默认）/ Brief（摘要 + 可展开的引用） |

四种组合都有用途：换设备继续对话（全量 + 完整）；派出一个干净的子任务（全量 + Brief）；委派给不做复制的节点（仅本地 + Brief）；隐私会话（仅本地 + 完整）。

```text
Brief {
  origin: { session, node, position }        // 可以回溯到原始 Log
  goal, state, decisions[{what, why}], constraints, open_questions, next_steps
  artifacts: [blob-ref + 说明]
  required_capabilities: [capability-id]
  return_to: { session }
}
```

Brief 由模型按 schema 生成，并在两端都写入 Log。摘要是有损的，所以目标节点可以通过 `invoke(origin, session_read, range)` 按需展开细节。

### 5.5 一致性：选择 CP

**立场**：宁可让某项操作暂时不可用，也不出现两个执行者。

**原则：可变的东西很小，交给共识；大的东西不可变或只有单一写者，异步复制即可。** 这是 git（不可变对象 + 可变引用）、Nix（不可变 store + profile 指针）、Datomic 和 Delta Lake 共有的结构。

| 数据 | 性质 | 一致性手段 |
|---|---|---|
| 成员、Binding/租约、期望状态、工作区 git 引用 | 小、可变、写入低频 | **控制平面**：Raft 复制状态机，线性一致 |
| Session Log | 单写者（Binding 持有者） | epoch 栅栏：副本拒绝旧 epoch 的追加；异步复制，副本只读 |
| 插件 KV | 单写者（所在 Binding 的持有者） | epoch 栅栏；故障转移时可能读到稍旧的快照，因此入站事件以外部 ID（如 Telegram `update_id`）去重 |
| blob、制品、git 对象 | 不可变、内容寻址 | 按哈希拉取，不存在一致性问题 |

结果是数据平面不存在任何需要合并的冲突：写者唯一，副本只会落后，不会分叉。

**控制平面**

- 投票者：常驻的完整节点，外加**见证者**（`enco witness`：同一个二进制的极小模式，只保存控制平面日志，不运行 Session，可以放在与 VPS 不同故障域的任何廉价常驻设备上）。手机和笔记本是 learner，读取控制平面，但不计入多数派。
- 3 个投票者可以容忍 1 个故障。只有一个投票者时照样正确，只是该节点宕机后无法做任何所有权变更（CP 本来就选择不可用）。
- 实现使用 openraft。共识属于"正确性极难"的地基，不自建。状态机只有几种记录，写入很低频：handoff、租约续约、部署、git 引用推进。

**两种所有权**

1. **Session：粘滞所有权。** 持有者不需要续约，与控制平面失联也能继续执行自己持有的 Session。所有权只能通过两种方式改变：持有者主动 handoff（控制平面 CAS），或主人确认的强制接管。因此不会自动出现两个执行者。
2. **渠道连接、定时任务：租约。** 例如 TTL 20s，每 7s 续约。
   - 续约失败的时间超过 `TTL − 安全余量` 时，持有者自行停止（关闭连接、停止触发）。
   - 控制平面只在旧租约按 leader 时钟过期后才授予新租约；leader 变更后，新 leader 先等满一个 TTL。
   - 于是渠道可以自动接管，同时任何时刻只有一个节点在轮询 Telegram 或持有 QQ 的 WebSocket。
   - **安全前提**：时钟速率漂移有界。租约计时必须使用计入休眠时间的时钟（Linux 用 `CLOCK_BOOTTIME`，Apple 平台用 `mach_continuous_time`）。Rust 的 `Instant` 在两个平台上都不计入休眠，笔记本合盖唤醒后会误以为租约仍然有效。每次对外发送前都要检查租约，而不只是在建立连接时检查。
   - 定时触发带有触发 ID `(schedule, occurrence)`，重复触发可以被识别。

**少数派一侧的行为**（CP 的代价，要在界面上说清楚）

- 可以继续执行自己持有的 Session，也可以新建 Session。全新的 key 不可能冲突，重连后再登记。
- 不能 handoff，也不能接管别处的 Session；发往别处 Session 的消息进入 outbox，重连后投递，以事件 ID 去重。
- 无法续约，所以会在 TTL 内释放自己持有的渠道租约，让多数派一侧接管。

**强制接管**（break-glass，需要主人确认）

- 确认界面显示"已复制到的最后位置"，让主人知道可能丢失哪段尾部。
- 如果旧持有者其实还活着（是分区而不是宕机），它重连后看到更高的 epoch 会立即停止；它尚未复制的尾部作为 `divergent_branch` 保留，并在主线的下一 Round 对模型可见。这是系统中唯一可能产生分叉的路径，而且只能由主人明确授权。

**外部副作用**：租约保证内部只有一个执行者，但外部服务看不到我们的 epoch。服务支持幂等键时，使用 `(session, epoch, round, call-id)`；`OutcomeUnknown` 语义保持不变。

**典型流程（VPS 宕机）**：Telegram 租约在 TTL 内由桌面接管，主人仍能联系到 Agent → 发往 VPS 持有的 Session K 的消息进入队列 → Agent 通过渠道通知"K 的所有者 VPS 不可达，最后复制位置为 …，是否强制接管？" → 主人确认 → 桌面以 epoch+1 继续执行 K。

### 5.6 与自我修改的交汇点

| 交汇点 | 机制 |
|---|---|
| 迁移后插件是否可用 | 按哈希拉取制品，在目标节点上按准入规则（§4.10）计算快照；不满足的能力不进入快照，这一点写入 Log，模型可见 |
| 在哪里构建 | `plugin_build` 是一个可被 invoke 的能力 |
| 修复一次是否处处生效 | 期望状态在控制平面，健康状态按节点记录 |
| 版本偏差 | 节点描述中包含宿主版本、Log 格式版本和 WIT 版本；handoff 前检查兼容性；新增字段对旧节点可以忽略 |
| 修复路径 | 同伴节点互为 supervisor；宿主切换需要主人确认 |

### 5.7 信任与安全（单一所有者）

- 节点身份使用 ed25519 密钥对；配对时通过 QR 码或短码交换公钥，此后一律使用两两认证的连接。
- Space 永远只属于主人：完整节点默认把全部能力导出给 Space，这与"能力放开"一致。按节点收紧导出范围的接缝保留在能力注册处。
- 密钥（API key、bot token）按接收节点的公钥加密后在 Space 内复制，这是渠道故障转移的前提；单个密钥可以设为仅本地。
- 远程 invoke 在被调方留下审计记录（来源节点、Session）。
- 主人确认请求会发送到主人所有可达的界面和渠道。

### 5.8 传输

- 每对节点之间一条认证的双工连接（WebSocket over TLS 或 QUIC），帧类型只有 `call / result / stream / sync / handoff / raft / heartbeat`。
- 本地调用与远程调用使用同一种能力调用形状，只有失败语义不同。
- 可达性交给载体层。**实时路径不承载任何不在持久路径中的事实。**

---

## 6. 内核要点

- **Session actor**：每个 Session 一个 tokio task，顺序处理 Inbox。
- **Log**：SQLite（WAL），键为 `(session, epoch, seq)`；blob 内容寻址。
- **"模型可见 ⟺ 已记录"只走一条构造路径**：唯一的构造者是本 Session 的 composer。它输出的 ContextPlan（带来源的完整请求）经内核校验后按内容寻址记录，然后原样交给 Provider；Attempt 记录所用的 composer 代际与 Provider 代际。composer 的确定性由契约测试检验（同样的输入必须得到同样的输出），运行时不做重复构造。授权 header 不属于被记录的内容（§7.1）。
- **注册即 RAII guard。**
- **扩展点只有两种形态**：Observer（对原始事实只读，维护自己的派生状态，形态为转移，见 §3.6）和少量内核定义的策略点（`inbound.preprocess`、`tool.gate`、`round.compose`）。`round.compose` 每个 Session 绑定一个，另外两个策略点每个节点各一个；它们都有原生的出厂实现，由安全模式使用。
- **记忆**：内核不知道记忆，它是宿主经 Tool（写入与搜索）和 Context（召回）两个端口接入的能力。
  - 权威是宿主 SQLite 中的记忆记录，由唯一的归属者写入（§3.6）；更正替换记录，遗忘删除记录。记忆仍会出现在当时的 Session Log 中，Log 不改写。
  - 检索索引（TriviumDB：向量 + 按两字切分的中文 BM25 + 图）是权威的函数，可以随时删除重建；embedding 经 Provider 插件调用，与模型调用走同一条边界。
  - **索引提名，权威裁决**：命中的每一条都回到权威读取当前内容后才能进入上下文，所以过期的索引只影响召回质量，不会让已更正或已遗忘的内容重新出现。
  - 置顶记忆按确定的优先级参与每轮预算，不依赖检索，也不依赖某次摘要恰好提到它们；其余按最新的输入召回。置顶不保证无条件纳入，超预算可以省略，并在 ContextPlan 中记录原因。embedding 不可用时退化为关键词召回，并作为省略记录在计划中。
  - TriviumDB 依赖 mmap 与线程，不能进入 Wasm，所以检索索引是 Host 机制；召回与写入策略目前是原生实现，Wasm 工具与 Context 贡献就绪（P2）后可以改为插件，通过宿主导入使用索引。多节点下记忆的归属与复制在 P4 设计，数据形态已满足下面的可分布不变式。
- **主人确认（Approval）**：内核命令可以进入 pending 状态，向主人所有可达的界面和渠道投递确认请求；任一界面的应答写入 Log 后，命令继续执行。当前用于宿主切换和强制接管，将来用于显式开通的权限。它只是一种 Event 加一个等待中的命令，不是新概念。
- **工具语义**：参数归一化只在一处；`side-effect` 且结果为 `unknown` 时不自动重试；长结果落盘并返回预览。
- **取消**：`CancellationToken` 贯穿 Round；取消 = 发出信号 + 等待静止（§4.5）。

**单机阶段就遵守的"可分布"不变式**：

1. Event、Round、调用使用全局唯一 ID（ULID），接收方去重。
2. Log 键为 `(epoch, seq)`，单机时 epoch 恒为 1。
3. **顺序来自结构，而不来自时钟**：Log 的顺序由归属者分配的 `seq` 决定，Space 级记录的顺序由控制平面的日志位置决定。墙钟时间戳只用于展示，以及带有人类时间语义的事情（例如"明天九点提醒我"）。唯一依赖时钟的正确性规则是租约到期，它只在本机测量，使用计入休眠的单调时钟（§5.5），不跨节点比较时钟。
4. Session 带有 `binding: (node, epoch)`。
5. 能力 ID 为 `(节点, 插件名, 名称)`。
6. blob 与制品内容寻址。
7. composer 是确定性的纯函数，所用代际写入 Attempt 记录。
8. Round 边界是唯一的安全点。
9. Space 级的可变记录（Binding、期望状态、git 引用）同样遵循 §3.6：单节点时它们的归属者就是本机，由本地 SQLite 事务以带前置条件的方式写入。共识实现以及随之而来的 trait，在 P4 出现第二个实现时再引入，不为单一实现建立 trait。

这份清单只保留**事后补上代价很高的数据形态**（ID、日志键、Binding 字段、能力 ID、内容寻址、确定性组装）；机制在真正需要时再引入。

---

## 7. 认知负担：渐进式披露与手册

Agent 友好的核心是认知负担轻。这里有两个读者：运行中的模型（受上下文预算约束）和开发中的 Agent（受理解成本约束）。二者用同一套方法：**分层，按需展开，每一层都能指向下一层。**

### 7.1 上下文组装：正确性在内核，策略在 composer，线上格式在 Provider

前缀缓存是效率问题，不是正确性问题。把"披露只增不减"这类缓存规则放进内核，会让 SillyTavern 式的组装（它有意重排内容、按深度注入）处处与内核冲突。按"机制在内核，策略在插件"，内核不包含缓存逻辑。三方的分工如下：

| 层 | 负责 | 不负责 |
|---|---|---|
| **内核** | 正确性：原样记录实际发出的请求（按内容寻址）；把被披露的能力绑定到本 Round 钉住的代际；校验计划；安全模式 | 顺序、插槽、披露多少、缓存 |
| **composer**（每个 Session 绑定一个） | 策略：插槽模型、排序、预算与取舍、披露、压缩时机、为缓存保持稳定的前缀 | 线上格式 |
| **Provider** | 线上格式：把请求映射为服务商协议，包括缓存机制（缓存断点、原生延迟加载工具、缓存键） | 内容取舍 |

这与系统其他部分的分工是同一个模式：渠道那边，宿主持有传输并负责提交，插件负责协议语义；这里，内核持有记录与绑定，composer 负责内容策略，Provider 负责协议。

内核只提供四个机制：

1. **可用 ≠ 可见**：Round 快照钉住全部可用的能力，ContextPlan 中的披露集决定模型能看到哪些。**模型只能调用它被告知过的能力**：可调用 = 已披露（包括本 Round 内通过搜索展开并已记录的）∩ 快照。
2. **ContextPlan 就是准备好的请求**：composer 输出一组有序的请求片段，每段要么是内联内容（由 composer 生成，例如 SillyTavern 格式化后的文本），要么是对不可变内容的引用（Log 条目、blob、候选块）；此外还有披露集（每项带完整的工具定义，即模型看到的名称、描述与 schema；名称必须与注册表一致，§4.10）、被省略的条目及原因，以及每段的稳定性标注。内核只负责解引用，不包含任何排版逻辑；校验通过后按内容寻址记录，再原样交给 Provider。
3. **记录缓存事实**：每次 Attempt 记录 Provider 回报的用量，包括命中缓存的 token 数；上一份 ContextPlan 作为 composer 的输入。这样缓存优化可以测量、可以迭代，而内核本身不含任何缓存逻辑。
4. **声明的要求与安全模式**：Session 的配置可以声明要求，例如默认的 Agent 配置要求救生集常驻（`fs_*`、`shell_exec`、`plugin_status/deploy/rollback`、`manual_read`、`capability_search`）。内核据此校验计划，不满足的计划按 composer 失败处理：回退代际或进入安全模式。安全模式下的出厂 composer 永远带有救生集。角色扮演一类的 Session 可以不声明这项要求。

**缓存优化的分工。** composer 知道哪些内容是稳定的（这是语义），所以在片段上标注 `stable` 或 `volatile`。默认 composer 在一个请求系列内只追加披露，在压缩时开始新系列。Provider 知道怎样在线上利用稳定性（例如在稳定边界放置缓存断点、用原生的延迟加载保持工具列表不变）。内核记录命中率。换一个 composer 或 Provider，正确性都不受影响。

**SillyTavern 式组装。** 它是一个 composer 实现（prompt manager 的顺序、深度注入、宏展开，都是它内部的插槽模型），再加上若干 Context 贡献（例如按关键词扫描最近消息的世界书、作者注释）。内核不定义任何插槽，插槽是 composer 自己的词汇。Context 贡献描述候选块时使用一小组共享的种类（instruction / knowledge / memory / reminder / example / state），再加上由具体 composer 自行解释的不透明元数据。这与系统其他地方使用的两层契约相同：控制信息有类型，业务内容是受 schema 约束的 JSON。`round.compose` 是"每个 Session 一个"，而不是全局单例：Agent 会话使用默认 composer，角色扮演会话使用 SillyTavern 式 composer，两者可以并存。文本补全（instruct 模板）格式化属于 composer 与 Provider 之间的约定，通过 Provider 的扩展字段表达，目前不需要。

所有可披露的条目使用同一个三级模型：

| 级别 | 内容 | 例子 |
|---|---|---|
| L0 目录 | 名称 + 一句话 | 工具、Skill、插件、远程节点的能力 |
| L1 说明 | 完整描述与 schema | 工具 schema、SKILL.md 正文、插件手册 |
| L2 参考 | 细节与引用，按需读取 | 长结果文件、Skill 附件、Brief 的引用、CONTRACT 的某个小节 |

这是 Agent Skills 三级结构在所有条目上的推广。已有的长结果落盘和 Brief 引用都是 L2 的实例。

### 7.2 一份文档模型，两种读者

L0/L1/L2 的文本不单独维护，而是从源码文档中提取：

- 第一段 → L0 摘要（rustdoc 的惯例）
- 正文 → L1
- 带标题的小节与链接 → L2

同一段 `///` 既生成开发手册，也生成运行时的披露文本。只面向开发者的内容写在 `# Implementation` 小节里，提取给模型时剔除。这是 deepseek-harness "What the model sees" 的自动化版本：模型看到什么，由生成器从同一来源得出，并附上 token 估算写进 README，不再手写。

### 7.3 从 WIT 生成 CONTRACT.md

已核实：wit-parser 0.259 为 package、interface、type、function 保留 `Docs`，解析 `@since` / `@unstable` / `@deprecated` 稳定性标注，默认支持 serde，也能从已编译的组件中解码出 WIT；wit-bindgen-rust 0.62 与 wasmtime 的 bindgen 都会把 WIT 文档转成 rustdoc；另有 wit-bindgen-markdown 0.62 可作参考。

所以一段 WIT `///` 会流向四处：插件 SDK 的 rustdoc、宿主绑定的 rustdoc、CONTRACT.md、运行时手册。

生成器 `cargo xtask docs` 基于 wit-parser 自建，便于加入自定义检查：

- **CONTRACT.md**：包级文档作为不变式前言（插件被动、状态单写者、Round 钉住、outcome 语义），然后是各 world 的导入与导出，最后是各 interface 的类型与函数（签名、稳定性、摘要、标准小节）。
- **CONTRACT-CHANGES.md**：与上一个发布版本的 WIT 做比对，列出新增、弃用和签名变化。插件升级时，Agent 先读这一份。
- **契约 lint**：任何 WIT 条目缺少文档都会失败（相当于 `deny(missing_docs)`）；返回 `outcome` 的函数必须有 `# Outcome` 小节；`side-effect` 类函数必须有 `# Idempotency` 小节；同一主版本内出现破坏性签名变化也会失败；插件之间的接口只能使用值类型，每个函数必须返回 `result<T, call-error>`（§4.10）。

```wit
/// Deliver a message through this channel.
///
/// # Outcome
/// - `ok`: the platform accepted it; payload is the platform message id.
/// - `failed`: definitely not sent.
/// - `unknown`: the request left the host; check before retrying.
///
/// # Idempotency
/// Not idempotent. The host forwards `call-context.call-id` as an idempotency
/// key where the platform supports one.
deliver: async func(target: string, message: json) -> outcome;
```

### 7.4 手册的层级与生成物

| 层 | 内容 | 来源 | 位置 |
|---|---|---|---|
| L0 | 常驻指令 + 地图（去哪里找什么） | 手写，限字数 | `AGENTS.md` |
| L1 | 宿主与插件的契约 | WIT 生成 | `wit/CONTRACT.md`、`wit/CONTRACT-CHANGES.md` |
| L1 | 目录：工具、事件、配置项、能力需求、原生例外 | 代码生成 | `docs/generated/*.md` |
| L1 | 插件手册：提供的能力、工具表（effect、摘要、token 估算）、所需导入、能运行的节点、配置 | 构建元数据 + 手写小节 | `plugins/<name>/README.md` 的生成区 |
| L2 | 内核与 SDK 的公开 API 索引（条目、签名、摘要、`file:line`） | rustdoc JSON | `crates/<crate>/API.md` |
| L3 | 源码 | — | — |

- 公开 API 索引来自 rustdoc JSON，使用仓库固定的同一个 stable 工具链生成，不引入第二个工具链：`RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-Z unstable-options --output-format json" cargo doc --no-deps`（已在 rustc 1.98.1 上实测，输出 `format_version` 60，保留文档注释）。`RUSTC_BOOTSTRAP` 只在 xtask 的这一步中设置，并使用独立的 target 目录。JSON 格式随工具链版本变化，所以 xtask 使用与之匹配的 `rustdoc-types` 版本，工具链升级时一起更新。
- **API.md 本身就是公开 API 的快照**：它提交入库，kernel 或 SDK 的公开 API 一旦变化，就会以 API.md 的 diff 出现在评审中，`--check` 保证它不过期。不需要再引入 `cargo public-api`：所有文档，包括 API 差异，都走同一个"生成物 + `--check`"机制。
- HTML 版 rustdoc 照常生成，给人看。Agent 读 Markdown。

### 7.5 两种手册：源码手册与运行时手册

- **源码手册**描述 git 中的版本，由 `cargo xtask docs` 生成。
- **运行时手册**描述正在运行的代际：从制品中解码嵌入的 WIT，加上 `describe()` / `list-tools()` 的结果，以制品哈希为键存进制品库。
- CLI `enco manual <contract|plugin|capability> [--node X]` 和给运行中的 Agent 使用的工具 `manual_read`，读取的都是运行时手册。所以手册永远与实际执行的代码一致，并且按节点区分，因为不同节点可能运行不同的代际。

### 7.6 自动化流程与门禁

```text
cargo xtask docs            生成全部生成物
cargo xtask docs --check    CI / pre-commit：WIT lint、生成物是否过期、API 差异、字数与 token 预算
plugin_build                构建 → 解码 WIT 与元数据 → lint（文档完整性、描述长度）
                            → 写入运行时手册（按哈希）→ 更新插件 README 生成区（git diff 对 Agent 可见）
plugin_deploy               新代际的运行时手册随代际一起生效
```

生成区用 `<!-- generated:begin -->` / `<!-- generated:end -->` 标记，手工修改生成区会被 `--check` 拦下。**文档过期在这里是构建失败，而不是一个靠自觉维持的习惯。**

---

## 8. 移动端

- 手机是完整节点，也是控制平面的 learner。前台时可以持有 Session、使用本地工具（Android 用 Cranelift，iOS 用 Pulley）。
- **进入后台前主动 handoff**：策略把本机持有的 Session 交给常驻节点，从而绕开 iOS/Android 的后台限制。如果此时控制平面不可达，Session 留在手机上，在后台暂停，回到前台后继续。
- 设备能力（通知、定位、相机、剪贴板、分享、日历）以远程能力的形式导出；需要前台的能力在条件不满足时返回明确的错误。
- **离线（CP 语义）**：可以继续自己持有的会话、新建会话；发往别处会话的消息进入队列；不能接管别处的会话。
- 重计算不放在 iOS 本地，应当 invoke 到 VPS 或桌面（Pulley 在原型的 JSON workload 上约慢 34 倍，该比值不能外推为通用系数；真机上需要重新测量）。
- 个人侧载或 TestFlight 与公开上架是两个问题，后者需要单独评估。

---

## 9. 性能

- Wasm 开销相对模型延迟可以忽略，优化重点在数据搬运：传引用、按范围读、用 `stream<u8>` 传大对象。
- 控制平面写入很低频，不在热路径上：Round 开始时的 epoch 检查读的是本地副本，粘滞所有权也无需续约。
- 实时路径使用有界队列，慢订阅者被关闭，而不是拖慢数据源。
- 需要持续测量：改动到可用的端到端时间、handoff 延迟、渠道故障转移时间（约为 TTL + 连接建立时间）、冷启动、内存、反复部署后的资源残留。

---

## 10. 仓库结构

```text
crates/
  enco-core/     # 类型：Event、Message、Outcome、NodeId、Epoch；无 IO
  enco-kernel/   # Session actor、Round、ContextPlan 校验与记录、能力快照、代际、Binding、Approval；不依赖 wasmtime 与网络
  enco-host/     # Host 服务：SQLite、blob、fs、exec、检索索引与记忆、http/ws 连接管理、调度
  enco-wasm/     # wasmtime 嵌入、WIT 绑定、组件适配、构建与校验
  enco-space/    # 节点身份与配对、Peer 连接、控制平面（openraft）、日志复制、invoke/handoff
  enco-sdk/      # 插件 SDK：绑定封装、#[tool] 宏、host fake
apps/
  enco/          # 单一二进制：serve / witness / chat / plugin * / space * / status；组合根；原生管理通道
  mobile/        # 之后再做
xtask/           # cargo xtask docs [--check]：WIT → CONTRACT、目录、API 索引、插件 README 生成区、lint（§7）
plugins/
wit/             # *.wit + 生成的 CONTRACT.md / CONTRACT-CHANGES.md
docs/{AGENTS.md, decisions/, cookbook/, generated/}
scripts/check-crate-boundaries   # 依赖方向（§4.1）
```

依赖方向：`enco-core` ← `enco-kernel` ← `enco-host` / `enco-wasm` / `enco-space` ← `apps/enco`。内核只知道 `NodeId`、`Binding`、`ControlPlane` 和"把调用或事件交给某个节点"这几个 trait。

**crate 按概念划分，按触发条件拆分。** Enco 已经满足 Clean Architecture 的依赖规则：`enco-core` 是实体，内核端口与 WIT 是端口，内核是用例，`enco-host`、`enco-wasm` 与 Wasm 插件是适配器，`apps/enco` 是组合根；最外层的插件由 Wasm 物理隔离。因此不另建 domain / contracts / ports / application / adapter 这类通用分层 crate，也不引入 Repository、Service、DTO 这套与本文词汇并行的命名。crate 的名字对应 §3.2 的概念；只有出现明确的边界、独立的依赖成本或稳定的变化原因时才拆分，不为"看起来分层"而拆。

已知的第一个拆分点是 `enco-host`。它同时承载两个概念：**Host 机制**（Store、blob、KV、fs/exec/http 原语、检索索引、时钟）与**原生能力**（出厂 composer、救生工具、工作区上下文、记忆策略）。

- **触发条件**：WIT 第一次需要 `log`、`http` 之外的宿主导入（例如 `state-get`、`blob-put`、`exec`，预计在 P1–P2）。那时插件运行时必须调用 Host 机制，而当前的依赖方向不允许 `enco-wasm` 依赖 `enco-host`。
- **拆分后的形态**（名称到时再定）：

  ```text
  enco-core ← enco-kernel ← enco-host     Host 机制
                           ← enco-wasm     插件运行时：经 WIT 把 Host 机制交给插件（依赖 enco-host）
                           ← enco-native   原生能力（依赖 enco-host）
                           ← apps/enco
  ```

- **触发之前**保持模块级分隔，不预先拆分：提前拆分会迫使记忆的检索索引变成只有一个使用者的通用机制。

Agent 可读性约定：`AGENTS.md` 只写常驻指令和地图；README 中的 What the model sees / Token effect 由生成器产出（§7.2）；所有目录都是生成物，并设置 `--check` 门禁；决策记录放在 `docs/decisions/`；维护一张失败策略表，和一张"需要重启或主人确认的改动"表。

---

## 11. 路线图（以验收标准划分）

**P0 内核主干（单节点，遵守 §6 不变式）**：Session actor、Inbox、Round、SQLite Log、原生 CLI 管理通道、原生 fs/shell 工具、OpenAI-Compatible 与 DeepSeek Wasm Provider 插件作为出厂代际；Binding 等可变记录存放在本地 SQLite 中；`round.compose` 使用原生出厂策略（救生集 + 一行目录），输出 ContextPlan；记忆（SQLite 权威 + TriviumDB 派生索引 + 经 Provider 插件的 embedding；置顶记忆优先参与预算，其余混合召回）；一个能跨越重启的提醒；安全模式与原生管理入口；门禁脚本和 `cargo xtask docs --check`（WIT lint + CONTRACT.md）从第一天起生效。
验收：可以在 CLI 对话；`kill -9` 后 Round 的中断状态明确，副作用不会重复；一条简单记忆经过更正、压缩和重启后仍能查回，被更正的旧内容不再被当作当前事实；删除记忆索引后可以从权威重建；提醒在重启后按时触发；门禁能拦下"违反依赖方向"、"WIT 条目缺少文档"和"生成物过期"的提交。

**P1 可恢复替换**：制品库、每调用一实例、`ArcSwap` 快照、deploy/rollback/status、probe、试用代际晋升与自动回退；构建流程生成运行时手册与 README 生成区；接线与准入进入注册表提交，能力 ID 带插件名，插件不再自报身份，扩展字段由内核标注来源（§4.10）。
验收：通过故障注入矩阵，包括：
- 构建失败、WIT 不匹配、probe 失败、10% 的调用 trap；
- 部署会拆掉正在使用的接线（例如 Session 配置所用的 Provider）时被拒绝，并指出使用者；
- Round 内部署自身；部署过程中宿主崩溃；数据库已提交、快照尚未发布时崩溃；
- 两个插件同时部署，最终目录包含两次更新；v2 的迟到健康结果不影响 v3；
- 有状态工具的并发调用不丢失更新；写入后 trap 的调用不留下写入；
- Provider 返回 stream 后导出函数结束，流仍能完成，也能被取消；Provider 回退时，旧流迟到的数据不进入新的 Attempt；
- Provider 被改坏后能自愈；披露策略被改坏后，安全模式仍能调用文件、Shell 和部署管理。

**P2 Agent 自主改进**：`plugin_*` 工具、结构化诊断、SDK 宏、git 集成、`manual_read` / `capability_search`、跨插件接口导入与宿主转发（§4.10）；默认 composer 改为 Wasm 插件（包括缓存友好的披露策略），由 Agent 自己迭代，以 Attempt 中记录的缓存命中率作为观测指标。
验收：Agent 独立修复一个真实缺陷，并且**全程只读手册和插件源码，不需要读宿主源码**。这同时检验手册是否够用，以及双向不泄露是否成立。回退一个被导入的插件后，依赖它的插件退出快照，原因写入 Log 且对模型可见。**验证第一个原始动机。**

**P3 渠道**：Telegram（长轮询）、QQ OneBot（宿主持有 WebSocket）。
验收：同一连接上相邻的两个事件跨代际处理时，游标与协议状态不倒退；在"入站接纳"与"游标提交"之间注入故障，不会确认一条尚未持久化的消息；兼容的替换不断线，不兼容的替换按协议重连补收。承诺的范围：对可重放的来源做到至少一次交付 + 本地去重；出站结果要么已知，要么为 `unknown`。

**P4 Space v1**：VPS + 桌面 + 见证者；控制平面（openraft）、粘滞所有权、handoff、invoke、带 epoch 栅栏的日志复制、制品按哈希分发、期望状态、git 引用 CAS、渠道租约。
验收：任务中途 handoff 后继续；handoff 过程中断开连接不出现双 Round；杀掉 VPS 后 Telegram 租约在 TTL + ε 内转移到桌面，且全程不存在两个轮询者；少数派一侧的行为符合 §5.5。**验证第二个原始动机。**

**P5 手机节点**：learner 节点、后台前自动 handoff、设备能力导出、Brief 委派。
验收：iOS 应用在任务中被切到后台后，任务在 VPS 上继续；VPS 上的 Session 可以调用手机相机。

**P6 CP 加固**：Jepsen 式故障注入（分区、时钟漂移、休眠与唤醒、租约期间 leader 变更、旧节点复活），强制接管与 `divergent_branch`。

**插件生态（P4 之后，与 P5、P6 的先后按需要决定）**：外来插件的 `plugin_install` / `plugin_update`、采纳时的主人确认、宿主同时链接 `enco:plugin` 的历史版本、`enco:plugin` 1.0、`enco-sdk` 按 semver 发布（§4.10）。
验收：安装一个第三方插件，并由另一个插件导入它导出的接口；上游更新扩大导入时停在待确认状态，确认前仓库和运行中的代际都不变；宿主升级 `enco:plugin` 主版本后，未重建的第三方插件照常运行；Agent 修好一个外来插件的缺陷，经主人确认后向上游提交 PR。

**之后**：Android 本机构建（Root/Shizuku + Ubuntu 中本机编译插件）、宿主自更新（主人确认；自主权限以后显式开通）、覆盖网络路由、Edge 节点（ESP32）、系统代际的整体回滚。

P3 与 P4 之间没有依赖，顺序取决于先需要日常渠道还是多设备。

---
