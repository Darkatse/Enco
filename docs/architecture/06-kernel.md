# 6. 内核要点

本章是[架构文档](../Architecture.md)的一部分，概念、术语和统一规则以总纲 §3 为准。

- **一个节点只有一个进程**：同一个 `ENCO_HOME` 同时只由一个进程持有，凭证是独占的文件锁，由操作系统在进程退出时释放；锁文件永不删除，否则新旧进程会各锁一个文件。socket 只是通信入口，不表示所有权。这是 §3.6 规则一在节点上的应用。
- **Session actor**：每个 Session 一个 tokio task，顺序处理 Inbox。
- **Session 监督树**（Erlang 的 supervisor 与 monitor）：Session 可以把任务委派给子 Session，父子关系在双方的 Log 中都有记录。子 Session 回报之后仍然存在，父子之间可以多轮往来。一个没有父 Session 的根 Session 与它的全部后代构成一棵树；子 Session 创建时继承根的 ID，此后不变。它只复用已有的机制：
  - 创建：内核命令 `delegate(父, profile, context)` 创建带 `parent` 的子 Session，首个 Event 是 Brief；`context` 取 `brief` 或 `full`，即 §5.4 的上下文轴。委派按 `(父 Session, call-id)` 幂等，崩溃恢复后不会重复创建。
  - 回报：子 Session 每结束一次 Run，内核就向父 Session 发出一个 `ChildReturned` Event，内容是这次 Run 的结局与最后一条回复。它与子 Session 的 `RunEnded` 在同一次提交中写入，经 outbox 投递，按 Event ID 去重（§3.6 规则二）；Run 因崩溃中断时，由恢复在记下中断的同一次提交中写入。由上层传播下来的取消不产生回报，因为发起者在等待静止时已经得到结果。
  - 消息：`session_send` 向同一棵树内的 Session 投递 Event，来源记为发送方（§3.3 规则一），发往树外返回 `failed`。子 Session 在执行中可以发送任意多条消息，回报则由内核产生、每次 Run 恰好一次，所以父 Session 能区分"还在做"和"这一段做完了"，这对应 Erlang 的消息与 monitor 的 `DOWN`。没有同步调用：父 Session 不在一次工具调用里等待子 Session 的答复，否则它的 actor 被占住，父子互等还会死锁。
  - 可见：父 Session 只看到以 Event 到达的内容，在它下一个 Round 开始时读到；需要细节时调用 `session_read`，读到的内容作为工具结果写入父 Session 的 Log。需要父 Session 处理的事用消息推送，只是查看进度就用 `session_read`，它不唤醒任何一方。
  - 取消：沿树向下传播，对象是该 Session 当前的 Run 与所有后代正在进行的 Run；发出信号并等待它们静止。
  - 限额：委派深度与同时处于 Run 中的子 Session 数由常量限制，整棵树的用量（包括消息唤醒的 Run）汇总到根 Session；超限的委派返回 `failed`。
  - 写入保持单线程（§3.6 规则一在工作区上的应用）：同一个工作区同一时间只由一个 Session 写入；需要并行写入时各用一个工作区，由父 Session 经 git 合并。这由工作区分配与 fs 工具保证，shell 可以写入任何位置，不在保证之内。无结构的蜂群可以在树内用 `session_send` 表达，但不作为设计目标。

  profile 是一份命名的 Session 配置，包括 composer、各用途的 Provider、声明的要求，以及一句面向模型的描述，写在 `config.toml` 中。`session_delegate` 的描述列出各 profile 的名字与描述，作为 L0 目录。指挥 + 专家、门面 + 幕后、审阅循环都是 profile、Skill 与 composer 的组合；如果某种拓扑需要修改内核，说明监督树还缺机制。
- **Log 与 blob**：Log 存于 SQLite（WAL），键为 `(session, epoch, seq)`，记下事实的骨架：发生了什么、结局、模型看到的文本、正文的哈希。条目永不删除，Transcript、压缩与恢复都依赖它。正文（ContextPlan、超长结果，以后的图片）存为内容寻址的 blob。
  - "模型可见 ⟺ 已记录"约束的是请求发出之前必须先记录；记录保留多久是主人的策略。任何 blob 都可以由清理插件删除，缺失时明确报告；进入 Transcript 的 blob 缺失时，按省略记入 ContextPlan。
  - 插件制品不是历史，存放在制品库（§4.8），由代际的回收管理（§4.5），不受清理策略影响。
- **"模型可见 ⟺ 已记录"只走一条构造路径**：唯一的构造者是本 Session 的 composer。它输出的 ContextPlan（带来源的完整请求）经内核校验后按内容寻址记录，然后原样交给 Provider；Attempt 记录所用的 composer 代际与 Provider 代际。composer 的确定性由契约测试检验（同样的输入必须得到同样的输出），运行时不做重复构造。授权 header 不属于被记录的内容（§7.1）。补全接口只由内核导入（§4.10），所以插件可以互相导入之后，这条路径仍然唯一。
- **注册即 RAII guard。**
- **扩展点只有两种形态**：Observer（对原始事实只读，维护自己的派生状态，形态为转移，见 §3.6）和少量内核定义的策略点（`inbound.preprocess`、`tool.gate`、`round.compose`）。`round.compose` 每个 Session 绑定一个，另外两个策略点每个节点各一个；它们都有原生的出厂实现，由安全模式使用。
- **记忆**：内核不知道记忆，它是宿主经 Tool（写入与搜索）和 Context（召回）两个端口接入的能力。
  - 权威是宿主 SQLite 中的记忆记录，由唯一的归属者写入（§3.6）；更正替换记录，遗忘删除记录。记忆仍会出现在当时的 Session Log 中，Log 不改写。
  - 检索索引（TriviumDB：向量 + 按两字切分的中文 BM25 + 图）是权威的函数，可以随时删除重建；embedding 经 Provider 插件导出的 `embedding` 接口调用。
  - **索引提名，权威裁决**：命中的每一条都回到权威读取当前内容后才能进入上下文，所以过期的索引只影响召回质量，不会让已更正或已遗忘的内容重新出现。
  - 置顶记忆按确定的优先级参与每轮预算，不依赖检索，也不依赖某次摘要恰好提到它们；其余按最新的输入召回。置顶不保证无条件纳入，超预算可以省略，并在 ContextPlan 中记录原因。embedding 不可用时退化为关键词召回，并作为省略记录在计划中。
  - TriviumDB 依赖 mmap 与线程，不能进入 Wasm，所以检索索引是 Host 机制；召回与写入策略目前是原生实现，Wasm 工具与 Context 贡献就绪（P2）后可以改为插件，通过宿主导入使用索引。多节点下记忆的归属与复制在 P4 设计，数据形态已满足 §3.7 的可分布不变式。
- **主人确认（Approval）**：内核命令可以进入 pending 状态，向主人所有可达的界面和渠道投递确认请求；任一界面的应答写入 Log 后，命令继续执行。当前用于宿主切换和强制接管，将来用于显式开通的权限。它只是一种 Event 加一个等待中的命令，不是新概念。
- **工具语义**：参数归一化只在一处；`side-effect` 且结果为 `unknown` 时不自动重试；内核在调用上下文中给出结果预算，能分页的工具在预算内按自己的单位停下并说明如何继续；超出预算的结果由内核截断为预览，全文存为 blob。Log 只记录结局与模型看到的文本，不另存工具的原始返回值。
- **取消**：`CancellationToken` 贯穿 Round；取消 = 发出信号 + 等待静止（§4.5），并沿监督树传播到子 Session。
