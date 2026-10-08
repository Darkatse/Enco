# 4. 插件系统

本章是[架构文档](../Architecture.md)的一部分，概念、术语和统一规则以总纲 §3 为准。

## 4.1 边界：双向不泄露

系统与插件互相泄露是插件系统最常见的问题。泄露有三个方向，每个方向都要有机制保证，而不能只靠约定：

| 方向 | 规则 | 保证手段 |
|---|---|---|
| 插件 → 宿主 | 插件只能调用 linker 提供的 WIT 导入；没有共享内存、没有宿主对象、不能 monkey-patch | **Wasm 的物理隔离**：组件只能访问 linker 中存在的导入 |
| 宿主 → 插件 | 内核与宿主不按插件或服务的身份写业务分支。服务之间的差异（例如渠道是否支持编辑、线程、打字状态）只能通过插件声明的能力表达。宿主按操作系统平台做适配是它的正常职责 | **结构保证**：kernel/host 不依赖任何插件 crate；插件身份在内核中只是不透明的 ID，没有插件枚举；唯一的契约是 WIT（lint、CONTRACT.md、破坏性变化检查），kernel/SDK 的公开 API 自 P2 起以 API.md 快照受 `--check` 约束（§7.4）。**行为验证**：P2 验收要求 Agent 只读手册与插件源码完成修复 |
| 插件 ↔ 插件 | 只能导入对方导出的 WIT 接口，由宿主经内核 `invoke` 转发；插件不引用其他插件的名字；KV 按插件隔离；不存在全局共享状态 | 插件只看得到自己导入的接口，接到哪个实现由主人配置决定（§4.10）；WIT 中没有能访问其他插件状态的导入 |

原生实现的例外必须登记在 `docs/decisions/native-exceptions.md`，写明缺失的宿主能力和迁移条件。

不扫描源码中的名字：那是对源码文本的断言，与测试准入相冲突；它会催生白名单，也无法证明边界正确（按身份分支的代码未必出现名字，出现名字的代码也未必越界）。"按身份分支"属于评审问题，由 `maintain-conceptual-integrity` 的检查清单承担。

术语：本文中"平台"只指操作系统或运行环境（Linux、macOS、Windows、Android、iOS），Telegram、QQ 等称为"渠道"或"服务"。一个词只表达一种含义。

效果：宿主升级不会破坏插件，因为契约只有 WIT；插件更新也不需要修改宿主。WIT 演进采用 Zed 的做法（宿主同时链接历史版本的 world 并做适配）；个人项目早期可以简化为"WIT 大版本变化时，由宿主触发全部插件重建"，Agent 自动完成；这项简化只在插件源码全部归主人时成立，开放插件生态（§4.10）之前必须改为同时链接历史版本。这就回应了"更新不及时"：没有任何一层需要等待另一层。

## 4.2 四层扩展机制

| 层 | 形式 | 何时使用 | 修改成本 |
|---|---|---|---|
| Skill | `SKILL.md` + 脚本 | 流程知识、胶水、一次性自动化 | 零构建 |
| MCP | 外部进程 | 复用现成生态 | 重启进程 |
| Wasm 插件 | Rust 组件（WIT） | 类型化、长期维护、常驻、可热替换、可跨节点 | 增量构建约 1s |
| 原生 crate | 编进宿主 | 内核、Host、CPU 密集计算、平台桥接、管理通道 | 宿主重建 + 主人确认 |

判据：能用 Skill 就不写插件，能写插件就不改宿主。

## 4.3 插件只响应调用

宿主持有 WebSocket、长轮询、定时器、Webhook 路由和插件状态；插件导出反应函数，按 §3.6 返回转移。

- **资源是声明式的。** `describe(config)` 返回插件需要的资源（连接、定时、Webhook 路由，每项有稳定的名称，宿主按插件身份划分它们的命名空间，所以改名不会触发重建），宿主负责对账：没变的保留，变了的重建，删掉的撤销。这与 §4.7 的"期望状态 / 实际状态"是同一个模式。用户创建的提醒这类持久任务是内核事实，通过 `schedule_create` 能力创建，不属于插件资源。
- **替换渠道插件时，默认在事件边界切换回调。** 如果新代码接续不了当前的协议状态（在 `describe` 中声明 `reconnect-on-upgrade`，或者连接选项发生了变化），就按协议重连并补收：Telegram 用 offset，Discord 用 RESUME。连接级的传输状态（例如流式解压）属于宿主传输层的连接选项，不是插件状态。**承诺的是可恢复，不是每次替换都零断线。**
- **渠道故障转移**：由新的租约持有者重建连接，从已复制的连接状态继续。

**会话映射与投递。** Session 属于 Enco，渠道中的聊天只是壳（§5.2 的界面位置）。

- **映射**：一个聊天（渠道、账号、聊天 ID）同一时刻挂在一个 Session 上，多个聊天可以挂在同一个 Session 上。新聊天默认挂到 `main`；主人用命令切换或新建 Session。命令不经过模型：写法由插件解析，含义由宿主执行。
- **主人**：只有主人的账号可以驱动 Session。发送者由宿主按配置核对，其他人的消息丢弃。群聊以后单独设计。
- **入站**：映射表与协议状态、入站游标一起归连接 actor 所有（§3.6）。入站消息经内核的入站入口投递进 Inbox，与连接状态的写入在同一次提交中完成，然后唤醒 Session。
- **出站**：模型显式发送是一次普通的工具调用。默认投递是壳在渲染 Log，与 `enco chat` 相同：连接 actor 以出站游标跟随它送过 Event 的 Session，与入站游标对称；每次投递先记下再发送，按 `outcome` 为每次逻辑投递保存一条最终结算，与出站游标在同一事务提交，发送途中崩溃记为 `unknown`，不自动重发。出站文本是 Markdown，由各生成方按 Markdown 书写；映射到渠道协议、协议解析、分段和错误分类由插件承担；连接归属者执行共同的路由与结算规则。结算引用 Session 与 Log 位置，不复制正文；`enco status.channels` 提供最近 failed / unknown 的读取入口。
- **由谁投递**：回复发往该 Session 中最近一条交互式 Event（CLI 或渠道）的来源；来源是 CLI 时不投递到渠道。提醒这类非交互 Event 触发的 Round 遵循同一规则。各个壳读同一份 Log 得出同一结论，彼此不需要协调。

## 4.4 契约（WIT 草图）

`completion`、`embedding` 和 `lifecycle` 已在 P1 定稿为 0.2。`tools` 随 P2 的第一个工具插件定稿，`channel` 在 P3 定稿，`state-get` 等到第一个有状态的插件出现时再加入。

```wit
package enco:plugin@0.2.0;

interface types {
  type json = string;
  record failure { code: string, message: string, retryable: bool }
  variant outcome { ok(json), failed(failure), unknown(failure) }   // unknown = 可能已执行
  record blob-ref { hash: string, mime: string, size: u64 }
  record call-context { session: string, round: string, call-id: string, node: string, result-budget: u32 }   // 结果可内联的字节数（§6）
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

/// 只由内核导入（§4.10）：每一次补全都是某个 Session 的一次 Attempt
interface completion {
  use types.{failure};
  /// 消息与工具调用是协议本身，用 WIT 类型表达；P0 的完整定义见 wit/plugin.wit 与生成的 wit/CONTRACT.md
  record request { messages: list<message>, tools: list<tool-spec>, max-output-tokens: option<u32> }
  complete: async func(settings: settings, request: request) -> result<completion, failure>;   // 非流式
  // 流式以新增函数加入：stream: async func(settings: settings, request: request)
  //   -> tuple<stream<completion-delta>, future<result<completion, failure>>>;
}

/// 宿主与插件都可以导入，例如记忆的检索索引
interface embedding {
  use types.{failure};
  embed: async func(settings: settings, inputs: list<string>) -> result<list<list<f32>>, failure>;
}

interface lifecycle {
  describe: func(config: json) -> result<description, failure>;   // 一句话摘要；以后加贡献、所需资源、状态作用域、reconnect-on-upgrade；不含插件名（§4.10）
  probe: async func() -> result<_, failure>;     // 自检；只依赖宿主持有的 fixture，不依赖外部服务
}

world base { import host; export lifecycle; }
// world chat-provider { include enco:plugin/base; export enco:plugin/completion; }   // 例如 DeepSeek：不支持嵌入，就不导出 embedding
// world telegram { include enco:plugin/base; export enco:plugin/channel; export enco:plugin/tools; }
// world summarizer { include enco:plugin/base; import alice:web/fetch@1.0.0; export enco:plugin/tools; }
//   插件之间没有按名字调用的导入：要使用其他插件的能力，就导入它导出的接口，由宿主转发（§4.10）
```

组件的 imports 就是它的全部需求：Host 接口由本机满足，其他接口接到已准入插件的导出（§4.10）。imports 可以从制品中读出，因此插件能否在某个节点上运行，是部署前就能回答的问题。

宿主不提供状态写入、事件投递、建立连接或注册定时这类命令式导入：它们都改为返回值或声明，契约因此更小。

## 4.5 制品、代际、调用与热替换

三种寿命不同的东西分别命名：

| 名称 | 是什么 | 寿命 |
|---|---|---|
| **制品**（Artifact） | 不可变的组件文件，按内容哈希寻址，复用 `InstancePre` | 永久（可垃圾回收） |
| **代际**（Generation） | 一次激活 = `(单调编号 n, 制品哈希, 配置引用)`。编号取注册表的全局提交序号，而不是每个插件各自计数，所以一个代际引用本身就是完整的，不依赖插件名或身份。同一份制品配上不同的插件配置（`describe(config)` 的输入）是不同的代际，所以凡是随插件配置变化的东西都属于代际：健康状态、运行时手册（§7.5），以及 Log 中 Attempt 与调用记录的代码引用。配置引用指向配置的某个版本，不内联其内容。调用参数（例如 Provider 的 base_url、model、options）由导入方的配置提供，属于接线，不属于代际（§4.6） | 从激活到被取代 |
| **调用**（Invocation） | 一次逻辑调用，独占一个 Store；作用域覆盖导出函数、它返回的全部 stream/future，以及最终结算 | 到结算为止（不是到导出函数返回为止） |

- **发布**：构建与校验可以并发进行，提交则只经过唯一的部署提交者，顺序是：制品完整写入 → 兼容性检查 → 在 SQLite 事务中提交代际记录 → 发布内存快照（`ArcSwap`）。如果在最后两步之间崩溃，重启后按数据库重建快照。Round 开始时执行 `load_full()`。
- **回收**：`Arc` 只负责代码的回收。在途工作由调用记录（它本来就在 Log 里）说明。
- **失败的代际**：如果某个 Round 钉住的代际被标记为失败，这个 Round 里对它的后续调用会明确返回 `failed(generation_revoked)`，要到下一个 Round 才看到回退后的目录。不会在同一个 Round 里悄悄换掉实现。
- **破坏性状态迁移**：由归属者在替换边界执行一次（相当于 Erlang 的 `code_change`），该插件因此放弃自动回退。其他情况只做 expand；SDK 的状态类型默认保留未知字段，这样旧代码回写时不会丢掉新字段。
- **取消** = 发出信号 + 等待静止：先 abort，再等待真正结束。Wasm 调用另外设置外部超时和 epoch 中断。

## 4.6 健康门控、恢复层级与宿主切换

```text
出厂代际（嵌入宿主二进制） ← 最近健康代际 ← 试用代际（probe + 前 N 次调用通过才晋升）
```

- 试用代际失败时自动回退。晋升和回退都携带期望的当前代际与状态（§3.6），迟到的结果改不动之后的路由。代际被标为失败时，注册表在同一次提交中记下原因，`plugin_status` 可以查到。回退由某个 Session 的调用触发时，同一次提交还向它投递 `deploy.rolled_back` Event。回退可能使依赖其导出的插件退出快照，原因写入 Log（§4.10）。
- **健康只统计能归因于代际本身的失败**：trap、契约违规、宿主持有的确定性 fixture 失败。网络中断、限流、凭证失效等外部失败由宿主分类，不会触发回退，因为它们在所有版本上都一样。"前 N 次没有 trap"只说明运行层面健康，不说明语义正确；语义正确靠 fixture 和契约测试。
- **出厂代际是内置的候选，不保证一定能用**：它仍然依赖兼容的 Host 接口、有效的配置和凭证，以及外部服务。
- **Provider 与 Attempt**：一个 Round 内可以有多次模型尝试（Attempt）。每次 Attempt 按用途（Reply、Compaction，委派落地后加上 Brief）使用 Session 配置为该用途接上的 `completion` 导出，所以同一个 Session 可以用大模型回复、用小模型压缩。每次 Attempt 记录完整的请求（ContextPlan 引用、Provider 代际，以及按内容寻址的调用参数中非密钥的部分：base_url、model、options 与密钥所在的环境变量名）和结果。只有成功结算的那次 Attempt 的工具调用会被分派，失败尝试迟到的 token 一律丢弃。Provider 失败后改用健康代际，就是开始一次新的 Attempt；如果旧代际无法表达当前请求，就在明确的边界上重新组装。**工具绑定以 Round 为单位固定，模型请求与适配器以 Attempt 为单位固定。**
- **安全模式**：所有策略点（`round.compose`、`inbound.preprocess`、`tool.gate`）改用原生出厂实现，Provider 使用出厂代际，只暴露救生集，不加载普通 Skill 和 Observer，使用最小系统提示词。正常路径连续失败时自动进入，主人也可以手动进入。它是 **Agent 自愈的唯一起点**。
- **机械恢复不依赖模型**：原生管理入口（本地 CLI/HTTP）可以列出代际、查看日志、停用插件、选择旧配置、进入安全模式和重启，模型完全不可用时也能操作。它不可被插件替换。Android 上需要一个不依赖 WebView 或聊天渠道的原生入口。
- 同伴节点可以通过 `invoke(node, shell_exec, …)` 诊断彼此。
- **宿主切换必须经主人确认。** Agent 可以自主完成宿主的构建、测试和预检，然后把"切换到宿主 vN+1"作为待确认命令提交（§6 Approval）；主人确认后由 supervisor 执行，健康检查失败则自动回滚到上一个二进制。自主切换的权限以后通过显式授权开通。宿主自更新整体排在路线图后期。
- **插件的热替换不需要确认**：它发生在健康门控与自动回退的保护下，恢复层级不依赖被替换的东西本身。

## 4.7 期望状态与系统代际

- **期望状态**放在控制平面，线性一致：`插件身份 → 代际`（制品哈希 + 配置引用）、按来源记录的已批准导入集合（§4.10），以及配置和 skills 的 git 引用。在任何节点上修好一个插件，其他节点都会收到。
- **实际状态**按节点记录：只有通过准入（§4.10）的代际进入快照，健康状态也按节点记录。
- **制品**按哈希向同伴拉取（类似 Nix binary cache）。
- **系统代际** = `(宿主版本, 活跃插件代际, 配置提交, skills 提交)`，可以命名、比较和整体回滚。新设备加入时采用期望状态，就得到同一套能力。

## 4.8 自我修改闭环

```text
~/.enco/                      ← git 仓库：主人的意图
  AGENTS.md  config.toml  plugins.lock  skills/  plugins/<name>/
  .gitignore                  ← /.data/ 与 /workspace/
  workspace/                  ← Agent 的工作目录，暂不纳入版本管理
  .data/                      ← 运行时状态，不纳入版本管理
    enco.db                   ← Log、Inbox、连接状态、KV、本地的控制平面副本
    memory.db                 ← 记忆（权威）
    memory-index/             ← 记忆索引（派生，可删除）
    blobs/                    ← 正文（§6）
    artifacts/<hash>.wasm     ← 制品库（内容寻址）
    enco.sock  enco.lock      ← 本地入口与单实例锁（§6）
```

意图与运行时状态由目录分开：仓库里看得见的是主人的意图，`.data/` 与 `.git/` 同属工具自己管理的目录。

- 命令（CLI 与 Agent Tool 同源，Tool 名为 `plugin_<动作>`）：`scaffold / build / test / deploy / status / rollback / logs / rename / remove`；外来插件另有 `install / update`。`scaffold`、`install`、`update`、`rename`、`remove` 维护 `plugins.lock`，`build` 登记尚未登记的目录（§4.10）。
- `plugin_build` 是只由装有工具链的节点导出的能力，手机上的 Agent 通过 `invoke(vps, plugin_build, …)` 构建，再按哈希取回制品。
- 构建诊断是结构化的，只返回前 N 条，完整日志存为 blob。构建以插件目录的内容快照为输入（构建前自动提交该目录，避免未提交的修改与产物对不上），每个代际记录 `{plugin_id, 源码快照, Cargo.lock 摘要, artifact_hash, wit_version, toolchain, config_ref}`。
- 构建时以空配置更新插件 README 中的生成区；部署时以新代际的配置生成它的运行时手册（§7.5、§7.6）。
- git 仓库在节点之间同步对象，`main` 的推进是控制平面上的 CAS（§5.5）。

## 4.9 状态与迁移

插件状态只放在宿主 KV 中，由其归属者串行写入（§3.6），并随 Space 复制；实例内存只作缓存。迁移遵循 expand-contract，并且必须兼容 Space 中较旧的节点（破坏性迁移见 §4.5）。**数据格式兼容、并发访问正确、业务语义兼容是三个独立的条件**：归属者保证第二条，expand 规则保证第一条，第三条靠契约测试。用户创建的持久任务是内核事实，不属于任何代际。

## 4.10 插件的命名、接线与生态

插件系统的依赖地狱通常来自四个条件：
- 共享的全局命名空间：JVM classpath、Garry's Mod 的 `_G`、Smalltalk 的 SystemDictionary。
- 通过修改别人来扩展：Minecraft 的 Mixin、Pharo 的方法覆盖、hook 返回值截断其他 hook。修改的先后会影响结果，于是需要加载顺序和优先级。
- 同名多版本加区间约束：可安装性因此是 NP 完全问题，Fabric 为此引入了 SAT 求解器。
- 没有人为一组插件的组合负责：整合包、Quicklisp dist、OTP release 都是事后的补救。

前三个在 Enco 中由结构消除：组件之间不共享任何东西，各自静态链接依赖；插件无法修改别人，也没有启动顺序（§4.3）；每个节点上每个插件只有一个活跃代际。第四个由系统代际承担（§4.7）。

剩下的是命名、接线、准入、演进和信任，各由一条规则处理，不引入新概念：名字与身份是插件的属性，接线与准入是注册表提交时的检查（§3.6 规则二），演进交给 WIT，信任复用主人确认。

**1. 名字与身份：人和模型用名字，记录用身份。**

- 插件名就是主人仓库 `plugins/` 下的目录名，使用 kebab-case（与 WIT 标识符同一规则）。目录不会重名，仓库又在 Space 内共享，所以插件名在 Space 内的唯一性由结构保证。
  - 安装时默认采用作者建议的名字，撞名即失败，由主人另选。
  - 原生能力与出厂插件的名字由宿主保留，例如 `fs`、`shell`、`memory`、`schedule`；它们的身份也由宿主固定。
  - 出厂插件的源码放在主人仓库中保留名字的目录下，`plugins.lock` 中的记录使用宿主固定的身份，所以嵌入二进制的出厂代际与 Agent 修改后构建的代际属于同一个插件，恢复层级（§4.6）不会因为修改而断开。
- 能力 ID 为 `(节点, 插件名, 名称)`，显示为 `web_search@vps`。
  - 模型可见的工具名是 `插件名_名称`：名称使用 snake_case，第一个下划线就是分隔符，总长受服务商上限约束（OpenAI 与 Anthropic 均为 64 个字符，由常量生成）。
  - 名字只由插件自身决定，不随其他插件的安装而变化。composer 决定披露哪些工具、披露到哪一级，但不决定名字。
- 契约名是 WIT 包名 `owner:package`。WIT 标识符不能包含 `.` 与 `/`，所以契约名使用 owner 而不是 URL。
- 插件身份是一个 ULID，在插件进入 Space 时由这个 Space 分配：主人自己的插件在 `plugin_scaffold` 或首次构建一个尚未登记的目录时进入，外来插件在 `plugin_install` 时进入。
  - 身份标识的是"这个 Space 里的这一次安装"，不是作品本身。作品由来源 URL 与 git 历史标识，fork 关系由共同的提交给出。
  - 身份不由作者写入，也从不随插件发布。作者写入的 ID 无法证明任何东西（任何人都能复制，fork 会原样继承），还会让上游内容决定主人的记账。将来需要不可伪造的作者身份时，应使用绑定在来源上的签名。
- 名字与身份记录在仓库根部的 `plugins.lock` 中，每个插件一项；外来插件另记来源（git URL + 已采纳的 commit），不绑定 GitHub、GitLab 或 Gitee 中的任何一个。
  - `plugins.lock` 由命令维护，不手改；`config.toml` 是主人手写的接线与设置。二者的分工与 Cargo.toml 和 Cargo.lock 相同。
  - 记录放在插件目录之外，所以插件目录里只有作品本身，发布时不会带出主人的记录。这由结构保证，不靠约定。
- 插件不自报身份。Assistant 消息所属的 Attempt 已经记录了所用的 Provider 代际，扩展字段只回放给同一身份的插件，所以改名不会打断回放，也不需要另外标注来源。

每类引用按它的读者选用标识：

| 引用方 | 标识 |
|---|---|
| 模型可见的工具名、CLI、配置接线、`plugin_status`、手册 | 名字 |
| Log 中 Attempt 与调用记录的代码引用 | 代际（不可变，§4.5） |
| 插件 KV、期望状态、代际历史、宿主持有的资源 | 身份 |
| 已批准的导入集合 | 来源（见 5） |

ULID 不进入 Log 条目。代际记录里有身份，需要跨改名追溯时，从代际查到身份即可，Log 因此保持可读。

操作的语义与文件系统一致：身份相当于 inode，`plugins.lock` 相当于文件系统的元数据，Agent 不需要学习新的模型。

- `plugin_rename`：在同一次提交里移动目录，并改掉 `plugins.lock` 中的名字。身份不变，状态、历史与连接随之保留。新名字在下一个 Round 边界生效，目录变化写入 Log；配置中按名字的引用需要跟着改，漏改由准入报出。
- 复制目录：副本在 `plugins.lock` 中没有记录，首次构建时由 Space 分配新身份，相当于一次 scaffold。
- `plugin_remove`：删除目录与记录。状态保留到显式清理为止，`git revert` 会同时恢复目录与原来的身份。
- 重新安装总是得到新身份，所以复用一个名字不会继承旧插件的状态或信任。
- 只执行 `git mv`：构建时发现一条没有目录的记录和一个没有记录的目录，于是拒绝，并提示改名用 `plugin_rename`、删除用 `plugin_remove`；这个状态也可能是删掉一个插件再新建了另一个，构建不猜测是哪一种。
- 仓库迁移：只改来源，名字与身份不变，信任需要重新确认（见 5）。

编进制品的名字（工具名、契约名）全局有意义；主人分配的名字与身份只在本 Space 内有效；插件从不引用另一个插件的名字。因此不会出现 crates.io、PyPI 那种"撞名只能改名"的局面：本地撞名只需换一个目录名，作者的代码不需要改动。

**2. 接线：依赖、槽位和贡献是同一件事。** 插件导出接口、导入接口，主人经配置把导入接到导出上。依赖、槽位和贡献的区别只在导入方要一个还是要全部，这由导入方决定。

- 插件的每个导入接到 Host，或恰好一个导出。候选唯一时自动接上；不唯一时由主人配置选择，否则准入失败，并列出候选。
  - 配置没有写明选择时，自动接上的结果由注册表记入 `plugins.lock`，之后新增的候选不会改变它。
- 内核与宿主也是导入方：

| 内核与宿主的导入 | 数量 | 选择写在哪里 |
|---|---|---|
| composer | 一个 | Session 配置 |
| `completion` | 每种 Attempt 用途一个 | Session 配置 |
| 策略点（`inbound.preprocess`、`tool.gate`） | 一个 | 节点配置 |
| 工具、Context 贡献、Observer、渠道 | 全部 | — |
| 宿主：`embedding`（记忆的检索索引） | 一个 | 节点配置 |

- 补全只由内核导入。只有内核能在请求发出之前把它记为 Attempt，并与 ContextPlan、钉住的代际绑定，所以插件导入 `completion` 会在准入时被拒绝，错误信息提示改用 `session_delegate`（§6）；`embedding` 等其他接口照常可由宿主与插件导入。这保证的是 Session 配置接上的模型只能经内核、以有记录的方式调用；插件经 `http` 自行调用外部模型，与调用其他外部服务相同，不在这条保证之内。
- 接口按导入方、约束以及能否单独提供来划分，不按实现方或线上协议划分。`completion` 与 `embedding` 的导入方和约束不同，也可以单独提供（DeepSeek 只导出前者），所以是两个接口。Chat Completions、Responses、Claude Messages 等协议对内核而言是同一件事，都导出 `completion`，差异留在各自的 Provider 插件里（§7.1）。按协议划分接口会迫使内核按服务身份选择调用方式，违反双向不泄露。
- 顺序由消费者决定：Context 的顺序归 composer，其余"全部"类的导入彼此可交换。插件不声明优先级或先后，也不能覆盖其他插件；要改变另一个插件的行为，就修改它的源码并部署新代际。
- 接线时做类型检查。同一个契约有多个实现是正常的；名字相同而类型不同的契约会被类型检查拦下，不需要另立规则。
- 跨插件调用由宿主转发，内部走内核 `invoke`：被调用方每次调用使用一个新 Store，照常结算 `outcome`，也可以跨节点。
  - 插件之间的接口只使用值类型。
  - 每个函数返回 `result<T, call-error>`，其中 `call-error` 由内核定义，包含 `unknown` 与 `unavailable`。
  - 这两条都由契约 lint 检查（§7.3）。
- 模型调用工具，代码导入接口。插件不按名字调用其他插件的工具。

**3. 准入：导入都接上的代际才进入快照。**
- 准入结果只由本节点各插件的活跃代际、接线和 Host 接口决定，与到达这个状态的路径无关。所以部署、回退和启动都从 Host 出发，对整份快照逐层计算：导入全部接到 Host 或已准入插件的导出，代际才准入。不满足的代际连同原因写入 Log，对模型可见。
- 插件之间的接线因此必须无环：环上的插件互相等待，都不会准入，原因中列出环上的插件。这与加载无关，插件没有启动顺序。两个插件互相需要，通常说明缺一个共同的归属者（§3.6）。
- 部署是有意的命令，不能拆掉正在使用的接线；被拒绝时，错误信息指出使用者和下一步。
- 回退是机械恢复，可以拆掉接线；受影响的插件退出快照并记录原因，因为恢复不能依赖被替换的东西本身。
- `plugin_status` 显示每个插件的导入、导出、使用者，以及它在各节点上是否准入、未准入的原因。

**4. 演进：只有代码消费者需要兼容规则。**
- 工具的读者是模型。模型每一轮都读取当前的定义（记录在 ContextPlan 中），所以工具可以随代际自由变化；改名会让引用它的要求失效，由校验报出。
- 接口的读者是代码，遵循 WIT semver：同一主版本内只增不改，破坏性变化就升主版本。wasmtime 的 linker 按主版本匹配。
- Enco 没有另外的兼容规则，也没有版本区间和求解器。每个导入只接一个导出，检查就是类型检查。

**5. 信任：只在外来代码进来时检查。** 插件作者不是 Space 的成员，他们的代码在主人的授权下运行，权限就是它的导入。Wasm 保证插件只能调用它导入的接口，而导入可以从制品中读出，所以不需要手写权限清单。

- 安装与更新都是采纳外来修订。采纳后的导入如果超出该来源已批准的集合，采纳就进入待确认状态（§6 主人确认），确认之前仓库和运行中的代际都不变。
  - 首次安装时已批准的集合为空，所以总要确认；更新只在扩权时确认，这与 Chrome 扩展的做法相同。
  - 已批准的集合按来源记录在期望状态中（§4.7），因为信任的是这个来源的代码。改名不影响信任；换成另一个来源的插件，或者仓库迁移，都从空集合重新确认。仓库迁移也可能意味着仓库换了主人，GitHub 的 repojacking 就发生在这种时候。
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
