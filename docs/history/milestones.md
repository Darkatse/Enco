# 里程碑与阶段存档

已验收里程碑的范围与手动核对记录，以及已完成的路线图阶段，按验收时的文档原样保留，之后不再更新。其中的类型名、章节号与规则可能已经改变；现行规则以 [spec/](../spec/) 与[架构文档](../Architecture.md)为准，往后的计划见[路线图](../Roadmap.md)。

## 里程碑

P0：M1 地基、M2 主干回路、M3 插件边界与 CLI、M4 持久性、M5 记忆、M6 压缩与提醒、M7 收尾、M8 审阅修正、M9 目录布局与 Telegram 渠道。P1（可恢复替换）：M10 请求查看、M11 代际与注册表、M12 契约 0.2、M13 profile 与接线、M14 健康门控。P1 之后：M15 缓存友好的请求与时间标记、M16 按消息锚定的上下文、M17 决策接口与记忆的相关性过滤、M18 周期定时与主人的时区。

| 里程碑 | 实现 | 暂不实现 |
|---|---|---|
| M10 | `Kernel::inspect` 与 `enco inspect`（04 §14）；`plan::validate` 与 `plan::resolve` 分开；A2 改用 `inspect`；`Inspection.settings` 与 M11 的记录字段一起加入 | 扩展字段按身份过滤（M12 才有身份） |
| M11 | 11 §1–§6、§8：`plugins.lock`、制品库、注册表、`Registry` 与导出表、`Runtime` 端口、Provider 与 Embedding 按调用传参、`CodeRef::Generation`、`AttemptStarted.settings`、出厂代际登记、部署与回退、准入、三个插件工具与 CLI；记忆经导出表取嵌入；schema 加 `plugins`、`generations` 表 | 试用与 probe：部署直接以 `healthy` 激活。调用参数仍来自 P0 形态的 `[provider]` 节点配置，经一个临时的 `default` profile 传入（M13 替换） |
| M12 | 07：WIT 0.2，`completion` 与 `embedding` 拆开，`describe(config)` 不再自报名字，`probe` 定义，`extension` 去掉 `provider`；enco-wasm 按导出建立 `InstancePre`；扩展字段在 `plan::resolve` 中按代际的身份过滤；插件重建；出厂插件目录改名 | Rust 的 `Lifecycle` 端口、`Loaded.lifecycle` 及 `probe` 调用（M14） |
| M13 | 12 与 08 §3：`[endpoint.*]`、`[profile.*]`、`sessions.profile`、`Kernel::set_profile`、`enco profile`、`ComposeInput.profile`、两个用途的预算；删除 `[provider]`、`[context]` 与 `SessionConfig`；同步更新 `enco init` 的模板、README 与 `examples/` 中的配置示例 | — |
| M14 | 11 §7：部署时 `probe`、`trial` 状态、`TRIAL_CALLS` 晋升、可归因失败的自动回退、`GenerationRolledBack` 事件、Attempt 回退后立即重试、安全模式用出厂代际；schema 升到 2 | 健康代际的降级 |
| M15 | 引入按钟点的时间标记与 `TranscriptItem.received_at`（04 §2、§5）；当时的末尾上下文排列已由 M16 替换，当前规则统一见 05 §4 | — |
| M16 | 05 §4：请求在最新条目处结束，不再追加末尾的上下文消息；输入前的说明（跨整点的时间、工作结局、召回的记忆）留在原位；composer 决定是否沿用上一份回复计划（`ComposeInput.previous_plan`）；`Candidate.standing`，内联消息记录 `sources`，inspect 返回原始计划；置顶记忆进 System 消息；Environment 给出时区；`Transcript.run_ends` 提供工作结局，文案由 composer 逐字记入计划（03 §1.8，04 §2） | 稳定性标注；按空闲时长开始新系列 |
| M17 | 07：契约 0.2.1 新增 `decision` 与 `decision-plugin`，enco-wasm 的 `Decision` 适配器与返回契约，出厂插件 `typesafe`（§4.4、§5）；04 §2 的 `Decision` 端口；11 §4.2、§6 的 `Exports::decision` 与 `Interface::Decision`；08 §3、§4 的可选 `[decision]` 与第三个出厂身份；06 §5.1 的相关性过滤；`plugins/README.md` 与 `examples/` 同步 | OpenAI 的 `/v1/decisions`；`tool.gate` |
| M18 | 03 §1.4、§1.5、§1.9、§3：`ScheduleRule`、`last`、`ScheduleState` 改为 Active / Done / Cancelled 并去掉 `ScheduleStateKind`、`Reminder.skipped`、`AttemptStarted.provider` 收窄为 `GenerationId`、Event 的叙述移出规范形态（回退通知由注册表写下）、`fire_schedule` 的前置条件、schema 升到 4；04 §2、§3、§5、§6.3、§10–§12：`Clock` 只返回 UTC、`TranscriptItem.event`、`KernelConfig.timezone`、`ComposeInput.timezone`、Scheduler 在内存中推出下一次触发并只补最近一次、`schedule_*` 的新参数与结果、两个常量；05 §4、§5：Environment 与时间标记按主人的时区、输入来源的说明；08 §1、§3、§4：必填的 `timezone`、`enco init` 检测本机时区、`enco schedules` 的显示；02 §3、§4：croner、chrono-tz、iana-time-zone；同步更新 `enco init` 的模板与 `examples/` 中的配置示例 | 定时租约与按 `(schedule, due_at)` 的 Inbox 唯一约束（P4）；修改已有的定时（取消后重建）；`at` 接受不带偏移的当地时间 |

每个里程碑结束时系统都完整可用。M11 到 M13 之间的过渡形态（临时 profile）只存在于代码中，规格只描述最终形态。

### 手动核对记录

A45 已于 2026-10-02 手动核对：使用临时 `ENCO_HOME`，由本机 HTTP 服务代替模型服务，另外核对了晋升、probe 拒绝、安全模式和下一个 Round 的规范消息。旧 schema 的库被拒绝启动，库本身不变。

A51 于 2026-10-08 核对：M16 的 OpenAI-Compatible 与 DeepSeek 制品均经新宿主部署、probe 与实际调用通过，调用记录指向部署的旧制品代际。真实 Jev 经守护进程的聊天请求路径判断中文记忆：饮品偏好保留，编辑器偏好被省略（p=0.05），阈值保持 0.5。

## 已完成的路线图阶段

**P0 内核主干（单节点，遵守 §3.7 的不变式）**：Session actor、Inbox、Round、SQLite Log、原生 CLI 管理通道、原生 fs/shell 工具、OpenAI-Compatible 与 DeepSeek Wasm Provider 插件作为出厂代际；Binding 等可变记录存放在本地 SQLite 中；`round.compose` 使用原生出厂策略（救生集 + 一行目录），输出 ContextPlan；记忆（SQLite 权威 + TriviumDB 派生索引 + 经 Provider 插件的 embedding；置顶记忆优先参与预算，其余混合召回）；一个能跨越重启的提醒；安全模式与原生管理入口；门禁脚本和 `cargo xtask docs --check`（WIT lint + CONTRACT.md）从第一天起生效。
验收：可以在 CLI 对话；`kill -9` 后 Round 的中断状态明确，副作用不会重复；一条简单记忆经过更正、压缩和重启后仍能查回，被更正的旧内容不再被当作当前事实；删除记忆索引后可以从权威重建；提醒在重启后按时触发；门禁能拦下"违反依赖方向"、"WIT 条目缺少文档"和"生成物过期"的提交。
状态：已完成。已实现系统的规格见 [spec/](../spec/)。

**原生 Telegram 渠道（P1 之前）**：把 `ENCO_HOME` 调整为 §4.8 的布局；原生 Telegram 适配器（长轮询、只接受主人的私聊）；聊天与 Session 的映射与切换命令、入站游标与 Inbox 同一次提交、出站游标与投递结算（§4.3）；登记为原生实现，P3 改为插件。它让主人可以日常使用，也让会话映射与投递的设计在 P1 定稿 WIT 之前经过真实使用。
验收：主人在 Telegram 私聊中对话，其他人的消息被丢弃；`/session` 切换与新建 Session，不经过模型；重启前后入站消息不丢也不重复处理；投递途中崩溃的回复记为 `unknown`，不重发；提醒沿用该 Session 最近的交互输入来源（CLI 输入之后不再自动投递到渠道）；最近失败与未知投递可以从 `enco status.channels` 追溯到 Log。实施规格见 [spec/10-telegram.md](../spec/10-telegram.md)。
状态：已实现，主人在 VPS 上日常使用，运行稳定。

**P1 可恢复替换**：换掉一个插件后出了问题，能回到上一个能用的版本。核心概念是代际（§4.5）。
- 部署与回退：制品库；注册表是唯一的部署提交者，用 `ArcSwap` 发布导出表；`deploy` / `rollback` / `status`，CLI 与工具同源；probe；试用代际自动晋升或回退（§4.6）。
- 记录：Attempt 记下所用的代际和调用参数（§4.5）。
- 模型选择：Provider 与模型从节点配置移到 Session 的 profile，每种 Attempt 用途各选一个。Provider 在每次 Attempt 时解析，工具在每个 Round 钉住（§3.2、§4.6）。
- 身份与接线：插件身份记在 `plugins.lock`，插件不再自报名字；出厂插件使用宿主固定的身份；扩展字段按 Attempt 记录的代际回放；接线与准入在注册表提交时检查（§4.10）。
- 契约：WIT 升到 0.2，定稿 P1 用到的五个接口：`types`、`host`、`completion`、`embedding`、`lifecycle`（§4.4）。

分五个里程碑实施：M10 请求查看（`enco inspect`）、M11 代际与注册表、M12 契约 0.2、M13 profile 与接线、M14 健康门控。各里程碑的范围与验收见 [spec/09 §4](../spec/09-gates-and-acceptance.md)。

验收：通过故障注入矩阵，包括：
- WIT 不匹配、probe 失败的制品被拒绝；试用代际的调用 trap 后自动回退，本次 Run 仍然完成，模型在下一个 Round 看到回退事件；
- 部署会拆掉正在使用的接线（例如 profile 所用的 Provider）时被拒绝，并指出使用者；
- 两个 Session 选用不同的模型时，各自的 Attempt 记录写明所用的模型与参数；同一个 Session 的回复与压缩可以使用不同的模型；
- Round 内部署自身：下一次 Attempt 用新代际，工具不变；部署过程中宿主崩溃；数据库已提交、导出表尚未发布时崩溃，重启后按数据库重建；
- 两个插件同时部署，最终目录包含两次更新；迟到的健康结果不影响更新之后的代际；
- Provider 被改坏后能自愈；安全模式下 Attempt 用出厂代际。

下面几项推迟了，原因相同：还没有第一个真正用到它们的地方。
- 工具接口与 `state-get`、`enco-host` 的拆分、运行时手册与 README 生成区、`plugin_build` 与 lint 表：在 P2 随第一个工具插件一起定稿。
- "同一份制品以两份配置部署为两个代际"：等 P3 的渠道插件有了配置再验收。
- 流式补全：有消费者时作为 `completion` 的新增函数加入，只改插件一侧。

**周期定时（P2 之前）**：`schedule_create` 在一次性的 `at` 之外接受 5 段 `cron`，可加 IANA `timezone`，省略时跟随主人的时区；主人的时区成为必填的配置项，时钟只提供 UTC 时刻（§3.7）。一次性提醒与周期定时是同一种 Schedule，仍是内核事实（§4.3），归属者是调度器（§3.6）。它让每日早报这类无人值守的任务可以日常运行：邮件与新闻经 `shell_exec` 调用命令行工具，流程写在提醒内容里，不需要新的插件或宿主能力。实施规格见 [spec/09 §4](../spec/09-gates-and-acceptance.md) 的 M18。
验收：每日定时按当地时间触发，重启后不重复，并按时继续；宕机跨过三次触发后重启，只补最近一次，Event 写明跳过两次；同一次触发不会因崩溃恢复而投递两次；取消后不再触发；跨过夏令时切换仍按当地时间触发；主人的时区改变后，跟随型的定时按新时区触发，指定了时区的不变。
