# Enco 路线图

路线图按验收标准划分阶段。设计见[架构文档](Architecture.md)，文中的 `§N.M` 指架构文档的章节。

**P0 内核主干（单节点，遵守 §6 不变式）**：Session actor、Inbox、Round、SQLite Log、原生 CLI 管理通道、原生 fs/shell 工具、OpenAI-Compatible 与 DeepSeek Wasm Provider 插件作为出厂代际；Binding 等可变记录存放在本地 SQLite 中；`round.compose` 使用原生出厂策略（救生集 + 一行目录），输出 ContextPlan；记忆（SQLite 权威 + TriviumDB 派生索引 + 经 Provider 插件的 embedding；置顶记忆优先参与预算，其余混合召回）；一个能跨越重启的提醒；安全模式与原生管理入口；门禁脚本和 `cargo xtask docs --check`（WIT lint + CONTRACT.md）从第一天起生效。
验收：可以在 CLI 对话；`kill -9` 后 Round 的中断状态明确，副作用不会重复；一条简单记忆经过更正、压缩和重启后仍能查回，被更正的旧内容不再被当作当前事实；删除记忆索引后可以从权威重建；提醒在重启后按时触发；门禁能拦下"违反依赖方向"、"WIT 条目缺少文档"和"生成物过期"的提交。
状态：已完成。已实现系统的规格见 [P0/](P0/)。

**P1 可恢复替换**：制品库、每调用一实例、`ArcSwap` 快照、deploy/rollback/status、probe、试用代际晋升与自动回退；构建时生成 README 生成区，部署时生成该代际的运行时手册（§7.5）；Attempt 与调用记录中的代码引用改为代际（§4.5）；Provider 与模型的选择从节点配置移入 Session 配置，并按 Attempt 用途选择，Attempt 记录调用参数（§3.2、§4.6）；WIT 定稿时把补全与嵌入拆成 `completion` 与 `embedding` 两个接口（§4.4）；接线与准入进入注册表提交，能力 ID 带插件名，插件身份由 `plugins.lock` 记录、不再自报，扩展字段按 Attempt 记录的代际回放（§4.10）。
验收：通过故障注入矩阵，包括：
- 构建失败、WIT 不匹配、probe 失败、10% 的调用 trap；
- 部署会拆掉正在使用的接线（例如 Session 配置所用的 Provider）时被拒绝，并指出使用者；
- 同一份制品以两份插件配置部署为两个代际时，健康状态、运行时手册和 Attempt 记录互不混淆；
- 两个 Session 选用不同的模型时，各自的 Attempt 记录写明所用的模型与参数；同一个 Session 的回复与压缩可以使用不同的模型；
- Round 内部署自身；部署过程中宿主崩溃；数据库已提交、快照尚未发布时崩溃；
- 两个插件同时部署，最终目录包含两次更新；v2 的迟到健康结果不影响 v3；
- 有状态工具的并发调用不丢失更新；写入后 trap 的调用不留下写入；
- Provider 返回 stream 后导出函数结束，流仍能完成，也能被取消；Provider 回退时，旧流迟到的数据不进入新的 Attempt；
- Provider 被改坏后能自愈；披露策略被改坏后，安全模式仍能调用文件、Shell 和部署管理。

**P2 Agent 自主改进**：`plugin_*` 工具、结构化诊断、`enco-sdk`（含宏）、内核与 SDK 的 API.md（§7.4，二者同时启用 `missing_docs`）、git 集成、`manual_read` / `capability_search`、跨插件接口导入与宿主转发（§4.10）、`plugin_build` 按仓库的 lint 表检查（§7.6）；默认 composer 改为 Wasm 插件（包括缓存友好的披露策略），由 Agent 自己迭代，以 Attempt 中记录的缓存命中率作为观测指标。
验收：Agent 独立修复一个真实缺陷，并且**全程只读手册和插件源码，不需要读宿主源码**。这同时检验手册是否够用，以及双向不泄露是否成立。回退一个被导入的插件后，依赖它的插件退出快照，原因写入 Log 且对模型可见。`plugin_rename` 之后插件的状态、代际历史与连接不变；只执行 `git mv` 时构建被拦下，并提示改用 `plugin_rename`。插件导入 `completion` 在准入时被拒绝。**验证第一个原始动机。**

**P3 渠道**：Telegram（长轮询）、QQ OneBot（宿主持有 WebSocket）。
验收：同一连接上相邻的两个事件跨代际处理时，游标与协议状态不倒退；在"入站接纳"与"游标提交"之间注入故障，不会确认一条尚未持久化的消息；兼容的替换不断线，不兼容的替换按协议重连补收。承诺的范围：对可重放的来源做到至少一次交付 + 本地去重；出站结果要么已知，要么为 `unknown`。

**P4 Space v1**：VPS + 桌面 + 见证者；控制平面（openraft）、粘滞所有权、handoff、invoke、带 epoch 栅栏的日志复制、制品按哈希分发、期望状态、git 引用 CAS、渠道租约。
验收：任务中途 handoff 后继续；handoff 过程中断开连接不出现双 Round；杀掉 VPS 后 Telegram 租约在 TTL + ε 内转移到桌面，且全程不存在两个轮询者；少数派一侧的行为符合 §5.5。**验证第二个原始动机。**

**P5 手机节点**：learner 节点、后台前自动 handoff、设备能力导出、跨节点委派。
验收：iOS 应用在任务中被切到后台后，任务在 VPS 上继续；VPS 上的 Session 可以调用手机相机。

**P6 CP 加固**：Jepsen 式故障注入（分区、时钟漂移、休眠与唤醒、租约期间 leader 变更、旧节点复活），强制接管与 `divergent_branch`。

**Session 监督树（需要多模型协作时实施，依赖 P1，不依赖 P3、P4）**：`session_delegate` / `session_send` / `session_read`、profile、取消传播与限额（§6）。
验收：父 Session 被取消后，所有子 Session 静止，未确认的副作用记为 `unknown`；子 Session 崩溃时父 Session 收到 `ChildEnded`；崩溃恢复后委派不会重复创建子 Session；超过深度限制的委派返回 `failed`。

**插件生态（P4 之后，与 P5、P6 的先后按需要决定）**：外来插件的 `plugin_install` / `plugin_update`、采纳时的主人确认、宿主同时链接 `enco:plugin` 的历史版本、`enco:plugin` 1.0、`enco-sdk` 按 semver 发布（§4.10）；采纳外来修订时检查其依赖的许可证与安全公告（cargo-deny）。这项检查要联网获取公告数据库，所以放在采纳流程和 CI 中，本地的 `cargo xtask check` 保持不依赖网络。
验收：安装一个第三方插件，并由另一个插件导入它导出的接口；上游更新扩大导入时停在待确认状态，确认前仓库和运行中的代际都不变；宿主升级 `enco:plugin` 主版本后，未重建的第三方插件照常运行；Agent 修好一个外来插件的缺陷，经主人确认后向上游提交 PR。

**之后**：Android 本机构建（Root/Shizuku + Ubuntu 中本机编译插件）、宿主自更新（主人确认；自主权限以后显式开通）、覆盖网络路由、Edge 节点（ESP32）、系统代际的整体回滚。

P3 与 P4 之间没有依赖，顺序取决于先需要日常渠道还是多设备。
