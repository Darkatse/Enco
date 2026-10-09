# Enco 路线图

路线图按验收标准划分阶段。设计见[架构文档](Architecture.md)，文中的 `§N.M` 指架构文档的章节。已完成的阶段（P0、原生 Telegram 渠道、P1、周期定时）见[存档](history/milestones.md)。

| 阶段 | 依赖 |
|---|---|
| P2 Agent 自主改进 | P1 |
| 脚本调用 | P2 |
| Session 监督树 | P1 |
| P3 渠道 | P1 |
| P4 Space v1 | P1 |
| P5 手机节点 | P4；跨节点委派还需要 Session 监督树 |
| P6 CP 加固 | P4 |
| 插件生态 | P4，以及 P2 的跨插件导入 |
| Windows 节点 | P0；参与 Space 还需要 P4 |

P2、Session 监督树、P3、P4 四个方向互不依赖，先做哪个取决于当时更需要什么。

**P2 Agent 自主改进**：Agent 能自己构建、部署和修复插件。
- 工具与状态：工具接口与 `state-get` 随第一个工具插件定稿，状态写入作为返回值由归属者提交（§3.6、§4.4）。WIT 从此第一次需要 `log`、`http` 之外的宿主导入，所以按 §10 拆分 `enco-host`。
- Round 钉住导出表：被撤销的代际在同一个 Round 内返回 `generation_revoked`（§4.5）。
- 构建与部署：`plugin_build`（含仓库的 lint 表与结构化诊断，§7.6）、`scaffold` / `rename` / `remove`、`enco-sdk`（含宏）、git 集成。
- 手册：内核与 SDK 的 API.md（§7.4，二者同时启用 `missing_docs`）、运行时手册与 README 生成区（§7.5）、`manual_read` / `capability_search`。
- 插件之间：跨插件接口导入与宿主转发（§4.10）。
- 原生实现改为插件：默认 composer 改为 Wasm 插件（包括缓存友好的披露策略、稳定性标注、按空闲时长开始新系列），由 Agent 自己迭代，以 `evals/` 的任务成功率为目标、Attempt 记录的用量（含缓存命中）为成本约束；fs/shell 工具经宿主的 fs 与 exec 导入改为 Wasm 插件，原生版本保留为安全模式的救生集；记忆的召回与写入策略按需改为插件（`docs/decisions/native-exceptions.md`）。
- 评测：`evals/` 的五道题各验证一个 P2 机制；运行器 `evals/runner/` 是黑盒客户端，只经 `enco` 的命令行驱动被测系统（§4.8）。题目格式见 [evals/README.md](../evals/README.md)。

验收：
- Agent 独立修复一个真实缺陷，并且**全程只读手册和插件源码，不需要读宿主源码**。这同时检验手册是否够用，以及双向不泄露是否成立。**验证第一个原始动机。**
- 回退一个被导入的插件后，依赖它的插件退出快照，原因写入 Log，对模型可见。
- 有状态工具的并发调用不丢失更新；写入后 trap 的调用不留下写入。
- 披露策略被改坏后，安全模式仍能调用文件、Shell 和部署管理。
- `plugin_rename` 之后，插件的状态、代际历史与连接不变；只执行 `git mv` 时构建被拦下，并提示改用 `plugin_rename` 或 `plugin_remove`；复制出的目录在首次构建时得到新身份。
- 插件导入 `completion` 在准入时被拒绝。
- `evals/` 的五道题都能端到端运行，基线写入运行结果；`adapter-drift` 经正式路径通过，让第一条验收可以重复检验。

**脚本调用（工具多到逐个调用的代价明显时实施）**：宿主的脚本沙箱（QuickJS 编译为 Wasm）、嵌套调用的分派与记录、脚本内的 `capability_search` 与 `manual_read`，以及 composer 的披露形态选择（§7.1）。
验收：一段脚本并行调用多个工具，模型只看到脚本的输出；脚本内搜索返回的能力可以调用，既未披露也未被搜索返回的被拒绝；被 `tool.gate` 拦下的嵌套调用在脚本中表现为失败；脚本在一次副作用调用之后抛错或被 `kill -9`，结算内容列出该调用及其结局，脚本不重跑。

**Session 监督树（需要多模型协作时实施）**：`session_delegate` / `session_send` / `session_read`、profile、Brief 作为 Attempt 用途、`ChildReturned` 回报、取消传播与限额（§6）。
验收：父 Session 被取消后，所有子 Session 静止，未确认的副作用记为 `unknown`；子 Session 崩溃时父 Session 收到 `ChildReturned`，父 Session 被取消后不会被子 Session 的回报重新唤醒；父 Session 在子 Session 回报后可以继续给它发消息；发往树外的 `session_send` 返回 `failed`；崩溃恢复后委派不会重复创建子 Session；超过深度限制的委派返回 `failed`。

**P3 渠道插件化**：渠道接口就绪，Telegram 适配器改为 Wasm 插件，原生适配器删除；接入 QQ OneBot（宿主持有 WebSocket）。
验收：同一连接上相邻的两个事件跨代际处理时，游标与协议状态不倒退；在"入站接纳"与"游标提交"之间注入故障，不会确认一条尚未持久化的消息；兼容的替换不断线，不兼容的替换按协议重连补收；同一份制品以两份配置部署为两个代际。承诺的范围：对可重放的来源做到至少一次交付 + 本地去重；出站结果要么已知，要么为 `unknown`。

**P4 Space v1**：VPS + 桌面 + 见证者；控制平面（openraft）、粘滞所有权、roam、invoke、带 epoch 栅栏的日志复制、制品按哈希分发、期望状态、git 引用 CAS、渠道与定时的租约（定时触发按 `(schedule, due_at)` 在 Inbox 去重）、记忆的归属与复制（§6）、数据库迁移。从 P4 起升级不丢记录；旧版本的节点和回滚后的宿主怎样读取新数据，也在这时确定。
验收：任务中途漫游到另一个节点后继续；漫游过程中断开连接不出现双 Round；杀掉 VPS 后 Telegram 租约在 TTL + ε 内转移到桌面，且全程不存在两个轮询者；少数派一侧的行为符合 §5.5；在桌面写入的记忆，Session 漫游到 VPS 后可以召回，更正与遗忘在各节点都生效。**验证第二个原始动机。**

**P5 手机节点**：learner 节点、进入后台前自动漫游回常驻节点、设备能力导出、跨节点委派。
验收：iOS 应用在任务中被切到后台后，任务在 VPS 上继续；VPS 上的 Session 可以调用手机相机。

**P6 CP 加固**：Jepsen 式故障注入（分区、时钟漂移、休眠与唤醒、租约期间 leader 变更、旧节点复活），强制接管与 `divergent_branch`。

**插件生态**：外来插件的 `plugin_install` / `plugin_update`、采纳时的主人确认、宿主同时链接 `enco:plugin` 的历史版本、`enco:plugin` 1.0、`enco-sdk` 按 semver 发布（§4.10）；采纳外来修订时检查其依赖的许可证与安全公告（cargo-deny）。这项检查要联网获取公告数据库，所以放在采纳流程和 CI 中，本地的 `cargo xtask check` 保持不依赖网络。
验收：安装一个第三方插件，并由另一个插件导入它导出的接口；上游更新扩大导入时停在待确认状态，确认前仓库和运行中的代际都不变；宿主升级 `enco:plugin` 主版本后，未重建的第三方插件照常运行；Agent 修好一个外来插件的缺陷，经主人确认后向上游提交 PR。

**Windows 节点（需要时实施，Unix 环境优先）**：本地端点在 Windows 上改用命名管道（tokio 不在 Windows 上提供 Unix domain socket），访问限定为当前用户；Shell 工具使用 PowerShell 7，工具描述按平台写明所用的 shell，输出按 UTF-8 解码；守护进程响应控制台关闭与系统关机；Unix 专用的测试按平台编译；CI 增加 Windows runner。参与 Space 之前补上计入休眠时间的时钟（§5.5）。这些都是宿主的平台适配（§4.1），不改变架构。
验收：`cargo xtask check` 在 Windows 上通过；CLI 经命名管道与守护进程完成对话，其他用户无法连接；`shell_exec` 正确返回中文输出。

**之后**：修改已有的定时（目前取消后重建）、`at` 接受不带偏移的当地时间、流式补全（有消费者时作为 `completion` 的新增函数加入，只改插件一侧）、健康代际的降级、决策接口的更多用途（`tool.gate`、OpenAI 的 `/v1/decisions`）、Android 本机构建（Root/Shizuku + Ubuntu 中本机编译插件）、宿主自更新（主人确认；自主权限以后显式开通）、覆盖网络路由、Edge 节点（ESP32）、系统代际的整体回滚；MCP 接入（某个需要的服务只提供 MCP 时实施，需要先回答：外部进程提供的工具定义怎样钉进代际，stdio 服务作为宿主持有的资源，工具标注怎样映射到 effect 与 `outcome`；采纳 MCP 服务等于完全信任，§4.10）。
