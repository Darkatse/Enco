# 里程碑与阶段存档

已验收里程碑的范围、验收场景与手动核对记录，以及已完成的路线图阶段，按验收时的文档原样保留，之后不再更新。其中的类型名、章节号与规则可能已经改变；现行规则以 [spec/](../spec/) 与[架构文档](../Architecture.md)为准，往后的计划见[路线图](../Roadmap.md)。

## 里程碑

P0：M1 地基、M2 主干回路、M3 插件边界与 CLI、M4 持久性、M5 记忆、M6 压缩与提醒、M7 收尾、M8 审阅修正、M9 目录布局与 Telegram 渠道。P1（可恢复替换）：M10 请求查看、M11 代际与注册表、M12 契约 0.2、M13 profile 与接线、M14 健康门控。P1 之后：M15 缓存友好的请求与时间标记、M16 按消息锚定的上下文、M17 决策接口与记忆的相关性过滤、M18 周期定时与主人的时区。P2：M19 评测运行器与基线、M20 契约 0.3 与凭证、M21 自我修改只有一条路径。

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
| M19 | 13：独立工作区的黑盒运行器 `eval-runner`（`run`、`compare`、`regrade`、`check`），中性 home 与白名单环境、mock 与拒绝代理、经 CLI 导出的快照、三种评分、逐题单侧 Fisher 检验与目标题的分层检验、`result.json`；08 §1、§7：`enco --version` 与 `enco send --wait`；02、09：运行器的依赖、lint 副本与门禁；`memory-update` 与 `daily-briefing` 细化并带齐自检案例，其余三道题的设计移入 M25 | 题目 `home/plugins/` 下夹具插件的构建（M24） |
| M20 | 07：契约 0.3，`describe` 返回摘要、配置 schema 与凭证，`settings` 删除 `api-key`；enco-wasm 在传输层按 origin 附加凭证、只在同一 origin 内跟随重定向、遮蔽交回插件的密钥值，加载时检查契约的主版本；04 §2、§6.4 与 06：三个端口与各种 endpoint 不再携带密钥的值，`Loaded.credential` 只含是否必需；11 §3、§6：回退的候选以当前出厂代际结尾，接线检查加上凭证；08 §3、12：启动时只检查变量已设置且非空；03 §1.6：`credential.unavailable`；三个出厂插件重建，provider-protocol 中的鉴权头与遮蔽删除 | 插件自带的凭证（名字与 origin）及代际级的凭证准入（M23）；路径前缀等放置方式（P3）；`config-schema` 的读者，即 README 与运行时手册中的配置项（M24、M25） |
| M21 | 11：配置代际与插件代际共用编号，库里只存内容；`TableOwner` 与求出声明，`ActiveConfig` 合起内容与声明，视图只经 `watch` 发布；`apply_config` 求出声明、在锁外探测新出现的使用、在锁内核对期望的活跃代际并提交；`config_divergence`；`set_profile` 与 `session_view` 经注册表；`deploy` 与 `apply_config` 接受代际编号；03：`generations` 父表、两种代际的子表与 `config`，schema 5，`config.invalid`；12：`[owner]`、`[run]`、`[endpoint.*]`、`[profile.*]` 由内核解析，`uses()`；06：`[memory]` 按名字引用 endpoint，索引身份与相关性过滤跟随视图；10：`[telegram]` 的归属者与跟随视图的监督任务；08：`enco config apply [--generation]`、`enco config rollback`、`enco plugin deploy --generation`；05：`config_apply` 进入 enco-host 与救生集，删除 `plugin_rollback` 工具；04：配置来自视图，Scheduler 订阅视图；删除 `KernelConfig`、`KernelDeps.profiles` 与 `arc-swap`；`enco init` 的模板、`examples/` 与 spec 13 的配置同步 | 插件代际的设置与 `[plugin.*]` 的归属者（M23）；`config.toml` 的源码历史（M24）；旧 schema 的迁移步骤，旧库重建（自动迁移在 P4）；A84 的手动核对推迟到 P2 结束（路线图） |

每个里程碑结束时系统都完整可用。M11 到 M13 之间的过渡形态（临时 profile）只存在于代码中，规格只描述最终形态。

### 手动核对记录

A51 于 2026-10-08 核对：M16 的 OpenAI-Compatible 与 DeepSeek 制品均经新宿主部署、probe 与实际调用通过，调用记录指向部署的旧制品代际。真实 Jev 经守护进程的聊天请求路径判断中文记忆：饮品偏好保留，编辑器偏好被省略（p=0.05），阈值保持 0.5。

A68、A69 于 2026-10-11 核对：两道题的案例全部符合期望，rubric 两次评判一致，空跑整道题失败；A/A 每题每变体 5 次，早报 5/5 对 4/5，记忆 5/5 对 5/5，没有仪器错误，未判倒退，保留 `k = 5`。

A70、A75 于 2026-10-11 核对：用临时组件加载一份导入 `enco:plugin/host@0.2.1` 的制品，加载失败，报错写明制品所用的版本与宿主链接的版本；按测试准入不保留文案断言。真实的 DeepSeek（`deepseek-flash`）与 Kimi（`k3-256k`）经凭证绑定各完成一次对话，保存下来的 Log、inspect 输出、宿主日志与 home 中的文件都没有出现密钥的值。

### 验收场景

M1–M21 的验收场景，"层"指主要在哪一层验证。编号在后续里程碑中延续。

| # | 里程碑 | 场景 | 层 | 必须观察到 |
|---|---|---|---|---|
| A1 | M1 | Store 的前置条件 | 集成 | 位置不连续的 Commit 被拒绝（`OutOfOrder`）；binding epoch 不符的 Commit 被拒绝（`Fenced`）；同一 EventId 投递两次返回 `Duplicate`，只产生一行 |
| A2 | M2 | 请求记录 | 集成 | 把 `AttemptStarted` 引用的计划与 Log 一起解析，得到的消息、工具定义与输出预算和 `ScriptedProvider` 实际收到的请求完全相同 |
| A3 | M2 | 一次工具调用后回复 | 端到端（与 A7 共用） | `fs_write` 真的写出文件；第二次 Provider 请求中包含对应的 Tool 消息；执行前预写由 A8 的崩溃恢复场景核对 |
| A4 | M2 | 未披露的工具与非法参数 | 集成 | 调用不存在的工具 → `ToolCallSettled(Failed tool.unavailable)`，没有 `ToolCallStarted`；参数不是对象 → `tool.invalid_arguments`；随后的 Round 正常进行 |
| A5 | M2 | Provider 重试 | 集成 | 一次可重试失败后成功：两对 Attempt，同一个 `plan` 哈希；不可重试失败：`RunEnded(Failed)` |
| A6 | M3 | 端到端对话 | 端到端 | `enco send` 得到预设回复；订阅收到的条目与 `enco log` 一致；`AttemptStarted.provider` 是 `GenerationId`，其编号在 `enco plugin status` 中是该插件的出厂代际 |
| A7 | M3 | 端到端工具调用 | 端到端 | wiremock 先返回 `fs_write` 调用、再返回文本：文件被写出；第二次 HTTP 请求体中的 `tool_call_id` 与第一次响应中的 id 一致 |
| A8 | M4 | 工具执行中 `kill -9` | 端到端 | wiremock 返回 `shell_exec`，命令为 `echo run >> marker.txt; sleep 60`；等 `marker.txt` 出现一行后 SIGKILL 守护进程并重启：`ToolCallSettled(Unknown interrupted)`、`RoundEnded(Interrupted)`、`RunEnded(Interrupted)`；`marker.txt` 仍然只有一行；再发一条消息，新的 Run 正常完成 |
| A9 | M4 | 模型请求中 `kill -9` | 集成 | Provider 阻塞时模拟崩溃；重启后出现 `AttemptSettled(Failed interrupted)`，没有任何 `ToolCallStarted` |
| A10 | M4 | 两个调用、第一个执行中崩溃 | 集成 | 第一个调用 `Unknown`（SideEffect）或 `Failed interrupted`（ReadOnly）；第二个调用 `Failed not_dispatched` |
| A11 | M4 | 取消 | 集成 | `shell_exec` 执行 `sleep 60` 时取消：迅速返回 `Unknown cancelled`，子进程已不存在；`RoundEnded(Cancelled)`、`RunEnded(Cancelled)` |
| A12 | M4 | 安全模式 | 集成 | 开启后，下一次 `AttemptStarted` 所引用的计划中 `tools` 恰好是救生集，计划中没有记忆，`RoundStarted.safe_mode` 为真；关闭后恢复 |
| A13 | M4 | 优雅关闭 | 集成 | Run 进行中调用 `shutdown`：以 `RunEnded(Cancelled)` 结束，`shutdown` 在所有任务退出后才返回 |
| A14 | M5 | 记忆的更正、遗忘与重启 | 集成 | 脚本：`memory_save`（"主人喜欢咖啡"）；下一次计划的记忆部分包含它；`memory_update` 改为"主人喜欢茶"；重启 Kernel 与 `Memories`；下一次新生成的记忆内容为"喜欢茶"；旧说明作为历史保留；`memory_forget` 后不再新召回，`memory.db` 中没有这一行 |
| A15 | M5 | 召回与"索引提名，权威裁决" | 集成 | 保存 20 条记忆，其中一条置顶：预算可容纳时，置顶的那条在无关的消息下也出现在计划中；与某条非置顶记忆字面相近的消息把它召回。随后让 `embed` 失败：更新一条记忆后，新召回的候选只含新内容，已记录的说明不改写；失败期间新保存的记忆以"尚未进入索引"出现；`plan.omitted` 含 `memory:semantic`。恢复 `embed` 后，下一次召回把它们写入索引。`embed` 阻塞时取消 Run：迅速以 `RoundEnded(Cancelled)` 结束，之后的召回照常把记忆写入索引 |
| A16 | M5 | 索引重建 | 集成 | 删除 `memory-index/` 后重启：召回照常，索引的节点数等于记忆数；改变配置的 `dimensions` 后重启：索引自动重建；`embed` 失败时启动：照常启动，记忆以"尚未进入索引"出现在召回中 |
| A17 | M5 | 端到端记忆 | 端到端 | wiremock 先返回 `memory_save` 调用保存置顶记忆、再返回文本，并模拟 `/embeddings`：工具循环没有新增自动 embedding 请求，第二次模型请求的 System 消息已包含置顶内容，`enco memory` 列出该记忆及其尚未索引的状态；再发一条输入，触发一次召回与索引同步，其请求仍包含记忆，`/embeddings` 的 `input` 包含记忆文本，`unindexed` 为空 |
| A18 | M6 | 压缩与重启 | 集成 | 用较小但记忆预算仍可容纳置顶条目的 `context_tokens` 迫使压缩（出现 `Compacted`）；重启 Kernel；之后的计划仍包含该置顶记忆，并且不引用被压缩隐藏的位置；压缩模型的窗口小于回复窗口时分几次压缩，每个 Run 都正常完成，每次压缩请求都放得进压缩窗口 |
| A19 | M6 | 提醒 | 集成 | `TestClock` 下，到期后恰好一次 `EventConsumed(Reminder)` 并触发一个 Run；在到期前"崩溃"并在到期后重启，提醒补发一次；创建一个已经过去的时间返回错误 |
| A20 | M6 | 端到端提醒管理 | 端到端 | 模型通过 `schedule_create` 创建提醒，CLI 可以列出并取消同一条提醒，取消后不再列出；到期与重启补发由 A19 的可控时钟场景核对 |
| A21 | M7 | 门禁有效 | 手动一次 | 临时让 enco-kernel 依赖 enco-host → `boundaries` 失败；删掉一个 WIT 函数的文档 → `docs --check` 失败；手改 `CONTRACT.md` → `docs --check` 失败。验证后还原 |
| A22 | M7 | 真实模型 | 手动一次 | 配置真实的 Provider 与 embedding 服务，`enco chat` 中：让它记住一件事、更正、在另一个话题中问起它、设一个一分钟后的提醒、执行一条 shell 命令；在 `enco log` 与 `enco memory` 中核对 |
| A23 | M8 | 单实例 | 端到端 | 守护进程运行时，第二次 `enco serve` 报错退出，不打开任何存储，第一个进程照常服务；第一个进程收到 SIGTERM 之后、退出之前，第二次启动同样被拒绝，既不打开存储，也不删除 socket 文件（用确定性协调拖住关闭，不依赖 sleep）；`kill -9` 之后重新启动成功（与 A8 共用） |
| A24 | M8 | 分页读取 | 集成 | 每行与说明行合计都能放入预算、总量超过预算的文件：结果不超过预算，只含完整的行，说明行给出 `continue at offset`，按它再读得到其余内容，`ToolCallSettled.full` 为空；单行加说明行也放不下时：返回这一行，由内核截断，`full` 指向 blob，说明行位于预览开头，继续位置仍然可见 |
| A25 | M8 | 工具结果持久化 | 集成（与 A2、A24 共用） | 执行工具并重启：结局与 `content/full` 保持不变，模型请求中的工具结果与重启前相同；大结果的全文从 blob 读取。原始返回值不进入 Log 由 `Settlement` 类型表达 |
| A26 | M8 | 本地时间 | 集成 | 主人的时区为 `America/New_York`，`TestClock` 在两个 Round 之间跨过夏令时切换：后一个计划中 Environment 显示切换后的偏移；切换前后的两条输入，时间标记各自按接收时的偏移渲染 |
| A27 | M9 | 目录布局 | 端到端（与已有场景共用） | `enco init` 之后运行时文件只出现在 `.data/`，`.gitignore` 忽略 `.data/` 与 `workspace/`；根目录的 `AGENTS.md` 出现在计划的 Standing instructions 一节 |
| A28 | M9 | Telegram 入站与命令 | 集成 | wiremock 模拟 Bot API：主人的私聊文本成为 `main` 中来源为 `Channel` 的 Event；其他用户、群聊与非文本更新只推进 offset；`/session work` 创建并切换，之后的消息进入 `work`；命令不产生 Provider 请求；`/cancel` 取消正在进行的 Run |
| A29 | M9 | 入站不丢不重 | 集成 | 一条更新提交之后停止并重启：下一次 `getUpdates` 的 offset 为 `update_id + 1`，Inbox 中这条消息恰好一条；在提交之前停止：重启后同一条更新被再次取回，并恰好接纳一次 |
| A30 | M9 | 出站投递 | 集成 | 最近交互输入的来源决定投递目标；Markdown 经原生富文本接口发送；超过 32768 字符的回复拆成多条，拼接后与原文相同；由 CLI 触发的回复不发往 Telegram；提醒触发的回复在重启后仍发往主人最近使用的聊天；聊天挂着别的 Session 时回复带独立成段的 `[<Session 名>]` 前缀；`sending` 已写入时重启不重发；每次逻辑投递都有最终结算，后续成功不覆盖此前 unknown / failed，`enco status.channels` 可读取其 Session 与 Log 位置；Run 失败或取消有终止通知 |
| A31 | M9 | 真实 Telegram | 手动一次 | 配置真实的 bot，在手机上完成对话、`/session` 切换、`/cancel`，并收到一分钟后的提醒 |
| A32 | M10 | 请求查看 | 集成 | 一个含工具调用与压缩的 Session：对每次 Attempt，`Kernel::inspect` 得到的 `request` 与 `ScriptedProvider` 当时收到的完全相同；压缩前后的两次 Attempt 各自解析出当时的历史；`enco inspect` 输出相同内容；计划的 blob 被删除后返回 `Blob` 错误而不是空请求 |
| A33 | M11 | 出厂代际与启动 | 集成 | 空库启动：每个出厂插件有一条 `healthy`、来源 `factory` 的代际并处于活跃，`plugins.lock` 含每个出厂插件的宿主固定身份；再次启动不新增代际；换一份出厂字节启动：新增一条出厂代际并成为活跃；主人部署过的插件在换出厂字节后活跃代际不变；`plugins.lock` 中出厂身份被改后拒绝启动 |
| A34 | M11 | 部署、回退与记录 | 集成 | `deploy` 之后下一次 `AttemptStarted.provider` 是新编号，`settings` 等于 endpoint 的参数且不含密钥；`rollback` 之后回到上一个健康代际，被回退的代际状态不变；再次 `rollback` 回到出厂代际；没有目标时返回 `NoRollbackTarget`；`plugin_deploy` 与 `enco plugin deploy` 的结果一致；两个部署并发提交后两条代际都在 `status` 中，活跃的是后提交的那条 |
| A35 | M11 | 准入 | 集成 | 把一份不导出 embedding 的制品部署到 `[embedding].plugin` 指向的插件：被拒绝，message 列出使用者 `embedding`，注册表不变；配置引用了没有登记的插件名：启动失败并指出 profile；不是组件的文件：`Rejected`，制品库中不留下记录以外的痕迹无妨 |
| A36 | M11 | 崩溃恢复 | 集成 | 模拟 `insert_generation` 已提交、导出表未发布的持久状态：重启后活跃代际是新代际，导出表与注册表一致；活跃代际的制品文件被删除后启动：自动回退到回退目标，被回退的代际标为 `failed`，`plugin_status` 显示加载失败原因，重启后保留 |
| A37 | M11 | 记忆经导出表 | 集成 | 部署嵌入插件的新代际后，下一次召回的 `embed` 由新代际的 `ScriptedProvider` 收到；回退后再次回到旧的 |
| A38 | M12 | 契约 0.2 | 端到端 | 两个出厂插件以 0.2 契约重建通过 `cargo xtask check`；`[embedding].plugin = "deepseek"` 时启动失败，错误说明它不导出 `embedding`；wiremock 让 DeepSeek 返回 `reasoning_content`：下一次对同一插件的请求体含它，经 `enco profile` 切换到 openai-compatible 后，同一 Session 的请求不含它 |
| A39 | M13 | profile | 集成 | 两个 Session 用不同 profile：各自的 `AttemptStarted.settings.model` 不同；同一个 Session 的回复与压缩用不同 endpoint：两类 Attempt 的 `settings` 与 `max_output_tokens` 不同，压缩请求放进压缩模型的窗口；`enco profile` 之后下一个 Round 用新 profile，正在进行的 Round 不变；Session 的 profile 不在配置中：`RoundEnded(Failed profile.unknown)`，`RunEnded(Failed)`，改回之后下一条消息正常 |
| A40 | M13 | 配置校验 | 端到端 | 缺少 `[profile.default]`、profile 引用不存在的 endpoint、输出上限不小于窗口、`api_key_env` 未设置，各自拒绝启动并指出位置；`enco init` 生成的模板能通过校验 |
| A41 | M14 | probe 与试用 | 集成 | probe 失败的制品：`Rejected`，注册表不变；probe 通过：代际为 `trial` 并活跃；累计 `TRIAL_CALLS` 次 `Ok` 后变为 `healthy`；`provider.network` 失败不计数、不清零，也不回退 |
| A42 | M14 | 自动回退 | 集成 | 试用代际的 `ScriptedProvider` 返回 `plugin.trap`：`AttemptSettled(Failed plugin.trap)` 之后，同一 Round 的下一次 `AttemptStarted.provider` 是回退目标，Run 正常完成；该代际状态为 `failed`，不再活跃，失败原因在 `plugin_status` 可查且重启后保留（embedding 触发也相同）；下一个 Round 消费一条 `GenerationRolledBack` 事件，模型请求中含它的规范消息；`enco chat` 显示回退提示 |
| A43 | M14 | 迟到的回报 | 集成 | 对已经不是活跃代际的编号回报 `Failed(plugin.trap)`：注册表不变；对 `healthy` 代际回报同样不变 |
| A44 | M14 | 安全模式 | 集成 | 部署新代际后开启安全模式：`AttemptStarted.provider` 是出厂代际；关闭后回到活跃代际 |
| A45 | M14 | 真实插件 | 手动一次 | 复制出厂插件源码到 `~/.enco/plugins/`，改一处可观察的行为（例如请求头），构建、`enco plugin deploy`，在 `enco chat` 中确认生效；再改成一个会 panic 的版本部署：第一次对话自动回退并收到回退提示，`enco plugin status` 显示失败的代际与当前活跃代际；`enco plugin rollback` 回到出厂代际 |
| A46 | M16 | 只追加 | 集成 | `TestClock` 在 Round 之间前进，相邻两次请求召回的记忆不同：同一 Run 的工具循环中、下一个 Run 中、重启 Kernel 之后，后一次回复请求都以前一次回复请求的全部消息开头，最后一条消息是最新的 Transcript 条目；上一份计划被清理后仍可继续，省略信息记录开始新系列的原因；修改常驻指令、置顶记忆变化、压缩或主人时区的偏移改变（夏令时切换）之后，System 消息随之改变，从头渲染 |
| A47 | M15 | 时间标记 | 集成 | `TestClock`：第一条输入带标记；同一钟点内的下一条不带；13:59 与 14:01 的两条中后一条带；相隔两小时的带；压缩之后可见的第一条输入带；压缩请求中同一条输入前有同样的标记；跨过夏令时切换后，每条输入的标记按它接收时的偏移渲染 |
| A48 | M16 | 记忆的位置 | 集成 | 置顶记忆在 System 消息中；与某条消息字面相近的非置顶记忆出现在这条消息之前的说明里；之后的请求中这条说明原样保留，同一条记忆在本系列中不再重复；`memory_update` 更正后在下一条输入再次召回时作为新的一行出现，改回原值也再次出现；inspect 中该行所属消息的 sources 与实际使用的候选一致；压缩后从头渲染时，仅有新输入的 Round 纳入本轮召回候选 |
| A49 | M16 | 工作中断 | 集成 | Run 以失败、取消、步数用尽或中断结束后，composer 在其后第一条输入的说明中写入结局，失败时含错误码与消息；一次接纳多条输入时不重复；旧计划原样保留说明，从头渲染时按 Log 位置还原归属；压缩与回复共享结局说明的渲染；inspect 直接读取已冻结的文字 |
| A50 | M17 | 相关性过滤 | 端到端 | wiremock 模拟 `/embeddings` 与 `/systemone`，召回两条非置顶记忆，`/systemone` 判其中一条无关：模型请求中只有另一条，`enco inspect` 的计划中有被滤掉那条的省略原因；`/systemone` 返回 500：两条都在请求中，计划中有一条 `memory:relevance` 省略 |
| A51 | M17 | 兼容与真实服务 | 手动一次 | 把 M16 构建的 0.2.0 出厂制品部署到新宿主：加载、probe 与调用照常；配置真实的 TypeSafe 服务，在 `enco chat` 中对比过滤前后的召回，据此确认 `MEMORY_RELEVANCE_THRESHOLD` |
| A52 | M18 | 周期定时 | 集成 | 主人的时区为 `America/New_York`，`TestClock` 前进：`0 8 * * *` 在当地 08:00 恰好投递一次 `Reminder`，`due_at` 是触发时刻，`skipped` 为 0，`schedule_list` 的 `next_due` 是次日当地 08:00；重启 Kernel 后不重复投递，并按时投递下一次；一次性提醒投递后状态为 `done`，不再列出 |
| A53 | M18 | 只补最近一次 | 集成、store | 停机跨过三次触发后启动：恰好一条 `Reminder`，`due_at` 是最近一次的触发时刻，`skipped` 为 2；`fire_schedule` 前移游标与投递进 Inbox 在同一事务中，任一写入失败时两者都不提交，所以提交前崩溃等于这次触发没有发生 |
| A54 | M18 | 夏令时 | 集成（与 A52、A26 共用） | `0 8 * * *` 跨过夏令时切换仍按当地 08:00 调度；重复小时内的输入按各自偏移显示。空档与重叠的 cron 求值规则沿用 croner（04 §10.1） |
| A55 | M18 | 跟随与固定 | 集成 | 主人的时区从 `America/New_York` 改为 `America/Los_Angeles` 后重启：未指定 `timezone` 的定时按洛杉矶时间触发，指定了 `America/New_York` 的不变；Environment 显示新时区，新计划中此前输入的接收时间与提醒的触发时刻按新时区叙述；旧计划原样回放 |
| A56 | M18 | 创建校验与取消 | 集成 | 无法解析的 cron、未知的时区名、永不触发的规则（`0 0 30 2 *`）、间隔小于 `MIN_RECURRENCE_INTERVAL` 的规则（`* 8 * * *`）、同时给出或都不给 `at` 与 `cron`：各自返回 `tool.invalid_arguments` 并说明原因，schedules 中没有新行，之后仍能成功创建；`schedule_cancel` 之后不再触发，`schedule_list` 与 `enco schedules` 不再列出，已进入 Inbox 的那次照常处理 |
| A57 | M18 | 配置与旧库 | 手动一次 | 缺少 `timezone` 或名称无效：拒绝启动并指出该项；`enco init` 的模板含检测到的时区并通过校验；schema 为 3 的旧库被拒绝启动，库本身不变 |
| A58 | M18 | 真实早报 | 手动一次 | 在 VPS 上配置 `timezone` 并手动迁移库；在 Telegram 的专用 Session 中让 Agent 创建每日早报（经命令行工具收邮件、读新闻），次日早上按时收到；停掉守护进程跨过触发时刻再启动，收到一次补发 |
| A59 | M19 | 等待 Run 结束 | 端到端 | `enco send --wait` 输出的 `run` 与 `end` 与 `enco log` 中这条输入被接纳后的第一个 `RunEnded` 相同；在一个 Run 进行中送入第二条，它的输出是接纳它的那个 Run 的结局 |
| A60 | M19 | 一次试验 | 评测 | wiremock 充当被测模型（先调用 `shell_exec` 执行 `mail list`，再回复）与评判：被测 home 的路径中没有 evals、trial 或题目名；`hidden/` 与 `task.toml` 不在 home 中；占位符已替换；`mail` 执行的是 home 中的夹具；快照中有 mock 的请求与状态、`tool-calls.json` 与对话文本；评判的请求与回复在 `judge/` 中，`reference` 中的日期已按开始时刻替换 |
| A61 | M19 | 离线重评分 | 评测 | 守护进程停止后 `regrade`：state 评分的结局不变；不带 `--judge` 时不向评判发请求；带 `--judge` 时按 `result.json` 中的评判配置重新评判，`judge/` 中的请求与回复随之更新 |
| A62 | M19 | 评判的结局 | 评测 | 评判回复的最后一行是 `UNKNOWN`：该评分项为 `fail`，原因注明 unknown；是其他内容，或评判返回 500 并重试 `JUDGE_RETRIES` 次后仍失败：该评分项为 `error`，没有其他评分项失败时试验记为 `instrument_error`，不计入分母；另有评分项失败时试验记为失败 |
| A63 | M19 | 只排除仪器的失败 | 评测 | 被测模型的地址不可达：Run 以 `provider.network` 失败，试验记为失败，原因写明失败码；mock 无法监听：试验记为 `instrument_error`，不计入分母 |
| A64 | M19 | 部署变体 | 评测 | 变体带一份 Provider 制品：第一条输入之前该插件的活跃代际就是它；`result.json` 记录制品哈希与 `generations` |
| A65 | M19 | 代理 | 评测 | 经 `shell_exec` 的 curl 访问 mock 的主机名得到 mock 的响应；访问其他主机得到 403，`network.jsonl` 中有这次访问 |
| A66 | M19 | 超时与越界 | 评测 | 被测模型阻塞直到超过 `timeout_secs`：Run 被取消，试验失败，原因是超时；工具调用的参数中出现运行目录的绝对路径：试验失败，原因是越界 |
| A67 | M19 | 比较与判定 | 单元、评测 | 13 §9 灵敏度表中的 p 值；两道目标题的分层检验；`accept` 与 `reject` 列出的原因；某道题的一个变体没有有效试验时结论为 `reject`，原因是没有有效样本；重评分发现题目哈希与原记录不同就拒绝，并指出是哪一道 |
| A68 | M19 | 题目自检 | 手动一次 | 配置真实的评判模型，`check memory-update daily-briefing`：全部案例符合期望，rubric 案例两次评判一致，空跑整道题失败 |
| A69 | M19 | A/A 基线 | 手动一次 | 配置真实的被测模型，`compare` 以同一个变体为基准与候选，跑两道题各 k 次，不指定目标：端到端完成；被判为倒退的题数写进 13 §11；结果作为基线保存 |
| A70 | M20 | 契约 0.3 | 端到端 | 三个出厂插件以 0.3 重建，`cargo xtask check` 通过；基于 0.2 的制品加载失败，message 写明制品所用的版本与宿主链接的版本（手动核对一次） |
| A71 | M20 | 密钥只在传输层 | 集成 | wiremock 模拟 Chat Completions：请求带有 `Authorization: Bearer <环境变量的值>`；服务在错误响应的正文中回显这个值时，`AttemptSettled` 的失败消息中是 `[redacted:<变量名>]`；Log 与 `enco inspect` 的输出中都找不到这个值 |
| A72 | M20 | 重定向 | 集成 | 同一 origin 的 307：跟随，第二个请求带着凭证，补全成功；跨 origin 的 302：Attempt 以 `provider.bad_request` 失败，message 中有 302，另一个 origin 的 mock 没有收到任何请求 |
| A73 | M20 | 接线中的凭证 | 集成 | endpoint 写了 `api_key_env` 而插件没有声明凭证：拒绝启动，错误写明 endpoint 与插件；deepseek 的 endpoint 没有 `api_key_env`：拒绝启动；openai-compatible 的 endpoint 不写 `api_key_env`：正常启动，请求不带 `Authorization`。把一份不声明凭证的制品部署到带 key 的 endpoint 所用的插件：`Rejected`，注册表不变 |
| A74 | M20 | 回退落到出厂代际 | 集成 | 活跃代际的制品无法加载、也没有可加载的更早健康代际：启动后插件的活跃代际是当前出厂代际，无法加载的代际标为 `failed` 并记下原因。手动 `rollback` 与自动回退使用同一份候选，不另验 |
| A75 | M20 | 真实服务 | 手动一次 | 经凭证绑定与真实的 DeepSeek、Kimi 对话；`enco log` 中找不到密钥的值 |
| A76 | M21 | 首次登记 | 集成 | 空库启动：`config.toml` 登记为第一个配置代际，`enco status` 中活跃配置代际是它；首次登记没有发出任何模型请求；再次启动不新增配置代际；全新 home 的 `config.toml` 无效：拒绝启动，错误指明是哪一张表 |
| A77 | M21 | 每张表的归属者 | 集成 | 缺少 `[owner] timezone`、profile 所选的 endpoint 没有写窗口、`[memory] embedding` 所选的 endpoint 没有写维度、出现不属于任何归属者的表（包括 `[plugin.web.config]`）：应用配置各自被拒绝，错误指明那一张表，不登记任何代际 |
| A78 | M21 | 真实调用 | 集成 | 回复 endpoint 的模型名改成 mock 拒绝的名字：整次被拒绝，错误指出那个 endpoint；回复 endpoint 换成拒绝带工具请求的服务：被拒绝；只改压缩 endpoint：只对它发一次不带工具的请求；没有改变的 endpoint 不被请求；结果的 `probes` 列出每次调用 |
| A79 | M21 | 应用后生效 | 集成 | 改了 profile 所选的 endpoint：下一个 Round 的 `AttemptStarted.settings` 是新的，正在进行的 Round 不受影响，不重启；改了 `[owner] timezone`：不重启，跟随型定时按新时区推算（与 A55 的期望相同）；改了 `[memory] relevance`：下一次召回用新的 endpoint；改了 `[memory] embedding` 所选 endpoint 的模型，或只改它的 `base_url`：都不重启，下一次召回前索引按新的身份重建，`index.json` 记录新的身份；只改 endpoint 的 `api_key_env`：索引不重建；改了 `[telegram] api_base`：不重启，渠道停止后按新表连接，之后的消息经新的地址收发 |
| A80 | M21 | 在用的 profile | 集成 | 删掉一个仍有 Session 在用的 profile 再应用：整次被拒绝，错误列出这些 Session；用 `enco profile` 切走之后重新应用，成功 |
| A81 | M21 | 期望的活跃代际 | 集成 | 用确定性协调拖住应用配置的真实调用：期间同一插件部署了新代际，应用配置返回 `Conflict`，什么都不写；期间一个试用代际晋升，应用照常成功。同一插件的两次部署交错提交：后提交者 `Conflict`，活跃的是先提交的那条 |
| A82 | M21 | 重启只恢复 | 集成 | 改了 `config.toml` 但没有应用就重启：活跃配置代际不变，启动日志与 `enco status` 报出 `unapplied`；`config.toml` 写坏后重启：节点照常启动；去掉某个 endpoint 的环境变量后重启：节点照常启动，用到它的调用以 `credential.unavailable` 结束；`[telegram]` 的 token 缺失：渠道不启动，`status.channels` 的 `stopped` 写明原因；活跃配置代际中的 `[memory]` 在当前宿主下解析不了（测试直接写入库）：节点照常启动，`status.config.invalid` 列出这张表，召回不使用索引（06 §3.2），应用一份有效的配置之后恢复 |
| A83 | M21 | 撤销也是登记 | 集成 | `plugin_deploy` 以代际编号重新部署：新代际的制品与那个代际相同，状态为 `trial`；`config_apply` 以上一个代际的编号 N 重新应用：产生一条新的配置代际，文档与代际 N 相同，`config.toml` 不变，`enco status` 报出 `superseded` 并写明新代际重新登记了 N；把 `config.toml` 改回之后不一致消失；`enco config rollback` 回到上一个配置代际，`enco status` 报出 `superseded`，原因是机械回退；救生集中有 `config_apply`，没有 `plugin_rollback` |
| A84 | M21 | 真实配置 | 手动一次 | 在 Telegram 中经 `config_apply` 换一次压缩模型并对话，再按 A83 的方式撤销 |

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

分五个里程碑实施：M10 请求查看（`enco inspect`）、M11 代际与注册表、M12 契约 0.2、M13 profile 与接线、M14 健康门控。各里程碑的范围见上文的里程碑表，验收见上文的验收场景。

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
