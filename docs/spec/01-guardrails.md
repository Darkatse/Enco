# 01 复杂度护栏

本项目最看重的是认知一致性：维护者（人或 Agent）用少量连贯的概念就能理解系统。实现中最常见的偏差不是"做少了"，而是"做多了"：为想象中的需求预留扩展点、为单一实现建抽象、为每种情况单独立规则。本文件列出整份规格都必须遵守的约束。**每个里程碑开始前重读一遍。**

## 1. 基本原则

- **YAGNI**：只实现本规格写明的东西。"以后可能需要"不是理由。
- **一种事情一套规则**：新代码先找已有机制。如果发现需要为某种情况单独写一套逻辑，停下来提问。
- **机制在内核，策略在宿主**：内核（enco-kernel）只负责记录、绑定、校验、调度；排版、提示词、工具实现都在宿主（enco-host）或插件中。
- **归属与转移**（架构文档 §3.6）：每份可变状态只有一个写者（归属者）；需要原子性的写入组成一次 `Commit`，由归属者提交。
- **Fail fast**：前置条件不成立或状态无法确认时，返回带上下文的错误。不要猜默认值，不要吞错误后假装成功。

## 2. 禁止的结构

| 禁止 | 原因与替代 |
|---|---|
| 事件总线、hook 系统、中间件链、插件管理器、服务定位器、依赖注入容器 | 依赖在组合根（`apps/enco`）一次性构造为具体的结构体（`KernelDeps`）传入 |
| 只有一个实现的 trait | 例外为本规格列出的内核端口（04 §2，含插件一侧与 WIT 接口一一对应的四个端口）与渠道协议适配器边界（10），它们位于真实的边界上 |
| 为简单结构体写 builder、`Default` 之外的工厂函数、`new` 带十个参数 | 用结构体字面量或参数结构体 |
| `Box<dyn Error>`、字符串错误、`unwrap()`/`expect()`（测试除外） | 每个 crate 用 `thiserror` 定义错误枚举；应用边界用 `anyhow` |
| 除规格列出的之外的重试、缓存、连接池、限流 | Provider 的可重试失败（04 §6.4），以及渠道轮询与确定失败的可重试分段（10） |
| 除规格列出的之外的后台任务 | 后台任务只有：每个 Session 一个 actor、Scheduler、Wasm epoch 计时器、本地协议的接受循环与每连接任务、渠道归属者及其轮询/订阅/发送助手（10）。记忆没有后台任务：索引在召回前对账（06 §3.3） |
| 超出 08 所列的配置项与环境变量（`ENCO_HOME`、`ENCO_LOG`、`api_key_env` / `token_env` 指定的变量），以及在本仓库的 crate 中定义 Cargo feature | 其余一律写死为常量，集中放在各 crate 的 `limits.rs` 中 |
| 在通用执行路径中按具体工具、Provider 或服务身份写业务分支 | 分派只处理 `CapabilityId`、`ToolSpec`、`CodeRef`。内核自己的 Schedule 命令通过普通 Tool 端口接入（04 §11），不增加专用分派路径 |
| 第二条请求构造路径；第二个事实来源 | 请求只由 composer 构造（04 §6.2）；事实只在 `enco.db`（Log、Inbox、schedules、connections、deliveries、plugins、generations、meta）、`memory.db`（记忆）与 `plugins.lock`（名字与身份）中。记忆索引与注册表的导出表是派生物，不是事实来源（06 §1、11 §4.2） |
| 为后续阶段预留空结构、空 trait、`todo!()`、注释掉的代码 | 到时候再加 |
| 宏（`macro_rules!`、过程宏）、泛型参数化的"通用框架" | 重复三次以上并且确实相同时，提取普通函数 |

## 3. 代码尺度

- 函数一般不超过 60 行，模块（文件）一般不超过 400 行。超出时**按职责**拆分，而不是按"层"拆分（不要出现 `types.rs` / `utils.rs` / `helpers.rs` / `common.rs`）。
- 一个文件回答一个问题。文件名就是它回答的问题：`recovery.rs`、`transcript.rs`、`plan.rs`。
- 公开 API 越小越好：crate 之间只暴露规格中列出的类型与函数，其余 `pub(crate)`。

## 4. 词汇锁定

代码、日志、错误消息、文档中统一使用下列词汇，**不要引入同义词**：

| 使用 | 不要使用 |
|---|---|
| Session | Conversation、Thread、Chat |
| Event（送进 Inbox 的输入） | Message（指输入时）、Request、Job |
| Inbox | Queue、Mailbox（代码中） |
| Log、Entry（Log 中的一条事实） | Journal、Record、History（指存储时） |
| Run（一个 Event 唤醒的若干 Round） | Task、Job、Session run |
| Round（一次模型请求 + 其工具调用） | Turn、Step、Iteration |
| Attempt（一次模型请求尝试） | Try、Call（指模型时） |
| ContextPlan、PlanItem | Prompt、Context、Payload |
| Composer、Composition | ContextBuilder、PromptManager |
| Candidate、ContextSource、Contribution（一次贡献） | ContextBlock、Fragment、Retriever |
| Memory（一条记忆）、Memories（记忆的归属者） | Note、Fact（指类型时）、MemoryStore、MemoryManager |
| 记忆索引（MemoryIndex）、召回（recall） | Vector store、Knowledge base、RAG、Retrieve |
| Capability、CapabilityId、Tool、ToolSpec | Function、Action、Skill |
| Outcome（`Ok` / `Failed` / `Unknown`）、Failure | Result（指工具结果时）、Status |
| Provider、Completion、embed | LLM client、Backend、Model service；vectorize、encode（指 embed 时） |
| CodeRef | Version、Implementation id |
| Schedule（定时：一串触发时刻）、ScheduleRule（`Once` / `Cron`） | Timer、Task、Job |
| SafeMode、Lifeline | Recovery mode、Core tools |
| Commit（一次原子提交） | Transaction（指领域对象时）、Batch |
| Store（持久化端口） | Repository、DAO、Database（指端口时） |
| Kernel | Engine、Core |
| Artifact（制品，内容寻址的组件文件） | Binary、Bundle、Package |
| Generation（代际，一次激活）、GenerationId（编号） | Version、Deployment、Release、Revision（指代际时） |
| Registry（注册表，代际的归属者）、Exports（导出表） | Catalog、PluginManager、Loader |
| Runtime（插件运行时端口）、Loaded（已加载的代际） | Instance、Component（指端口时） |
| Trial / Healthy / Failed（代际状态）、Factory / Deployed（来源） | Staging、Stable、Active（指状态时） |
| Profile、Endpoint | ModelConfig、Preset、Persona |
| ProviderSettings（调用参数） | Credentials、Options（指整组参数时） |

`Message` 只指发给模型的消息（`enco_core::Message`）。

"不要使用"一列只禁止把这些词当作同义词。第三方类型（例如 wasmtime 的 `Engine` 及其包装 `WasmEngine`、TriviumDB 的 `Database`）、架构文档确定的 crate 名（`enco-core`）、以及发给模型的自然语言提示词不受此限。

## 5. 提问协议

跨边界契约、持久格式、用户可观察行为与实施范围的变化，先向主人说明问题、可选方案、建议与理由，得到确认后再实施；不受影响的工作可以继续。例如改变 crate 职责或依赖方向、内核端口、WIT、本地协议、数据库模式、Log 格式、工具语义，或者新增依赖、配置项、长期后台任务。既定职责内的局部实现（私有类型与函数签名、模块拆分、测试辅助、错误消息措辞、日志内容）由实现者自主处理。不要把局部代码组织升级为架构决策，也不要以局部调整为名扩大范围。

## 6. 自检清单（每次提交前）

- [ ] 新增的类型、函数、文件，能否由规格中既定的职责与范围解释？
- [ ] 有没有为同一件事写第二套规则？
- [ ] 通用执行路径是否按具体工具、Provider 或服务身份写了业务分支？
- [ ] 有没有新增后台任务、配置项、依赖、trait？如果有，是否已提问？
- [ ] 错误是否带上下文传播，而不是被吞掉或变成默认值？
- [ ] 新增的测试是否守护可观察的行为或非平凡的不变式（09 §3）？
