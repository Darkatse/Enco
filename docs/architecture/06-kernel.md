# 6. 内核要点

- **Session actor**：每个 Session 一个 tokio task，顺序处理 Inbox。
- **Session 监督树**（Erlang 的 supervisor）：Session 可以把任务委派给子 Session，父子关系在双方的 Log 中都有记录。它只复用已有的机制：
  - 创建：内核命令 `delegate(父, profile, context)` 创建带 `parent` 的子 Session，首个 Event 是 Brief；`context` 取 `brief` 或 `full`，即 §5.4 的上下文轴。委派按 `(父 Session, call-id)` 幂等，崩溃恢复后不会重复创建。
  - 监视：子 Session 的 `RunEnded` 与发往父 Session 的 `ChildEnded` Event 在同一次提交中写入，再经 outbox 投递，按 Event ID 去重（§3.6 规则二）。子 Session 崩溃同样产生 `ChildEnded`。
  - 通信：`session_send` 向任意 Session 的 Inbox 投递 Event，来源记为发送方 Session（§3.3 规则一）。
  - 可见：父 Session 只看到以 Event 到达的内容；需要细节时调用 `session_read`，读到的内容作为工具结果写入父 Session 的 Log。
  - 取消：沿树向下传播，发出信号并等待所有后代静止。
  - 限额：委派深度与同时存在的子 Session 数由常量限制，整棵树的用量汇总到根 Session；超限的委派返回 `failed`。
  - 写入保持单线程（§3.6 规则一在工作区上的应用）：同一个工作区同一时间只由一个 Session 写入；需要并行写入时各用一个工作区，由父 Session 经 git 合并。无结构的蜂群可以用 `session_send` 表达，但不作为设计目标。

  profile 是一份命名的 Session 配置，包括 composer、各用途的 Provider、声明的要求，以及一句面向模型的描述，写在 `config.toml` 中。`session_delegate` 的描述列出各 profile 的名字与描述，作为 L0 目录。指挥 + 专家、门面 + 幕后、审阅循环都是 profile、Skill 与 composer 的组合；如果某种拓扑需要修改内核，说明监督树还缺机制。
- **Log**：SQLite（WAL），键为 `(session, epoch, seq)`；blob 内容寻址。
- **"模型可见 ⟺ 已记录"只走一条构造路径**：唯一的构造者是本 Session 的 composer。它输出的 ContextPlan（带来源的完整请求）经内核校验后按内容寻址记录，然后原样交给 Provider；Attempt 记录所用的 composer 代际与 Provider 代际。composer 的确定性由契约测试检验（同样的输入必须得到同样的输出），运行时不做重复构造。授权 header 不属于被记录的内容（§7.1）。补全接口只由内核导入（§4.10），所以插件可以互相导入之后，这条路径仍然唯一。
- **注册即 RAII guard。**
- **扩展点只有两种形态**：Observer（对原始事实只读，维护自己的派生状态，形态为转移，见 §3.6）和少量内核定义的策略点（`inbound.preprocess`、`tool.gate`、`round.compose`）。`round.compose` 每个 Session 绑定一个，另外两个策略点每个节点各一个；它们都有原生的出厂实现，由安全模式使用。
- **记忆**：内核不知道记忆，它是宿主经 Tool（写入与搜索）和 Context（召回）两个端口接入的能力。
  - 权威是宿主 SQLite 中的记忆记录，由唯一的归属者写入（§3.6）；更正替换记录，遗忘删除记录。记忆仍会出现在当时的 Session Log 中，Log 不改写。
  - 检索索引（TriviumDB：向量 + 按两字切分的中文 BM25 + 图）是权威的函数，可以随时删除重建；embedding 经 Provider 插件导出的 `embedding` 接口调用。
  - **索引提名，权威裁决**：命中的每一条都回到权威读取当前内容后才能进入上下文，所以过期的索引只影响召回质量，不会让已更正或已遗忘的内容重新出现。
  - 置顶记忆按确定的优先级参与每轮预算，不依赖检索，也不依赖某次摘要恰好提到它们；其余按最新的输入召回。置顶不保证无条件纳入，超预算可以省略，并在 ContextPlan 中记录原因。embedding 不可用时退化为关键词召回，并作为省略记录在计划中。
  - TriviumDB 依赖 mmap 与线程，不能进入 Wasm，所以检索索引是 Host 机制；召回与写入策略目前是原生实现，Wasm 工具与 Context 贡献就绪（P2）后可以改为插件，通过宿主导入使用索引。多节点下记忆的归属与复制在 P4 设计，数据形态已满足下面的可分布不变式。
- **主人确认（Approval）**：内核命令可以进入 pending 状态，向主人所有可达的界面和渠道投递确认请求；任一界面的应答写入 Log 后，命令继续执行。当前用于宿主切换和强制接管，将来用于显式开通的权限。它只是一种 Event 加一个等待中的命令，不是新概念。
- **工具语义**：参数归一化只在一处；`side-effect` 且结果为 `unknown` 时不自动重试；长结果落盘并返回预览。
- **取消**：`CancellationToken` 贯穿 Round；取消 = 发出信号 + 等待静止（§4.5），并沿监督树传播到子 Session。

**单机阶段就遵守的"可分布"不变式**：

1. Event、Round、调用使用全局唯一 ID（ULID），接收方去重；插件身份同样是 ULID（§4.10）。
2. Log 键为 `(epoch, seq)`，单机时 epoch 恒为 1。
3. **顺序来自结构，而不来自时钟**：Log 的顺序由归属者分配的 `seq` 决定，Space 级记录的顺序由控制平面的日志位置决定。墙钟时间戳只用于展示，以及带有人类时间语义的事情（例如"明天九点提醒我"）。唯一依赖时钟的正确性规则是租约到期，它只在本机测量，使用计入休眠的单调时钟（§5.5），不跨节点比较时钟。
4. Session 带有 `binding: (node, epoch)`。
5. 能力 ID 为 `(节点, 插件名, 名称)`。
6. blob 与制品内容寻址。
7. composer 是确定性的纯函数，所用代际写入 Attempt 记录。
8. Round 边界是唯一的安全点。
9. Space 级的可变记录（Binding、期望状态、git 引用）同样遵循 §3.6：单节点时它们的归属者就是本机，由本地 SQLite 事务以带前置条件的方式写入。共识实现以及随之而来的 trait，在 P4 出现第二个实现时再引入，不为单一实现建立 trait。

这份清单只保留**事后补上代价很高的数据形态**（ID、日志键、Binding 字段、能力 ID、内容寻址、确定性组装）；机制在真正需要时再引入。
