# 7. 认知负担：渐进式披露与手册

本章是[架构文档](../Architecture.md)的一部分，概念、术语和统一规则以总纲 §3 为准。

Agent 友好的核心是认知负担轻。这里有两个读者：运行中的模型（受上下文预算约束）和开发中的 Agent（受理解成本约束）。二者用同一套方法：**分层，按需展开，每一层都能指向下一层。**

## 7.1 上下文组装：正确性在内核，策略在 composer，线上格式在 Provider

前缀缓存是效率问题，不是正确性问题。把"披露只增不减"这类缓存规则放进内核，会让 SillyTavern 式的组装（它有意重排内容、按深度注入）处处与内核冲突。按"机制在内核，策略在插件"，内核不包含缓存逻辑。三方的分工如下：

| 层 | 负责 | 不负责 |
|---|---|---|
| **内核** | 正确性：原样记录实际发出的请求（按内容寻址）；把被披露的能力绑定到本 Round 钉住的代际；校验计划；安全模式 | 顺序、插槽、披露多少、缓存 |
| **composer**（每个 Session 绑定一个） | 策略：插槽模型、排序、预算与取舍、披露、压缩时机、为缓存保持稳定的前缀 | 线上格式 |
| **Provider** | 线上格式：把请求映射为服务商协议，包括缓存机制（缓存断点、原生延迟加载工具、缓存键） | 内容取舍 |

这与系统其他部分的分工是同一个模式：渠道那边，宿主持有传输并负责提交，插件负责协议语义；这里，内核持有记录与绑定，composer 负责内容策略，Provider 负责协议。

对话输入、回复与工具结果由规范格式还原；输入的接收时间、Run 的结束结局等事实由 Transcript 提供。Agent 对这些事实的叙述属于 composer，逐字冻结在计划中，不进入 core 的规范消息转换。内核提供上一份回复计划，是否沿用也由 composer 决定。

**协议差异留在 Provider。** Chat Completions、Responses、Claude Messages 等线上协议都导出同一个 `completion` 接口（§4.10）。多家协议共有的含义（例如图片输入、结构化输出）才进入规范类型，按 WIT semver 只增不改；只属于某一种协议的东西由 Provider 自行处理：
- thinking 签名与 reasoning 内容放进 `extension`，只回放给同一身份的插件（§4.10）；
- Responses 的 `previous_response_id` 作为 `extension` 返回，只当作缓存使用：Log 中始终有完整的请求，服务端状态失效时就发送完整请求；
- 服务端内置工具由 `options` 开启，它们的结果属于那次补全，随 AttemptSettled 记录。只允许效果限于那次补全的服务端工具（例如搜索、在服务商沙盒中运行代码）；会产生外部效果的（例如远程 MCP）必须作为 Enco 的 Tool 接入，经过 `tool.gate` 并按 `outcome` 结算。

内核只提供四个机制：

1. **可用 ≠ 可见**：Round 快照钉住全部可用的能力，ContextPlan 中的披露集决定模型能看到哪些。**模型只能调用它被告知过的能力**：可调用 = 已披露（包括本 Round 内通过搜索展开并已记录的）∩ 快照。
2. **ContextPlan 就是准备好的请求**：composer 输出一组有序的请求片段，每段要么是内联内容（由 composer 生成，例如 SillyTavern 格式化后的文本），要么是对不可变内容的引用（Log 条目、blob、候选块）。内联消息记录实际使用的候选来源（id 与原始文本哈希），供 composer 沿用与事后查看。此外还有披露集（每项带完整的工具定义，即模型看到的名称、描述与 schema；名称必须与注册表一致，§4.10）、被省略的条目及原因，以及每段的稳定性标注。内核只负责解引用，不包含任何排版逻辑；校验通过后按内容寻址记录，再原样交给 Provider。
3. **记录缓存事实**：每次 Attempt 记录 Provider 回报的用量，包括命中缓存的 token 数；上一份 ContextPlan 作为 composer 的输入。这样缓存优化可以测量、可以迭代，而内核本身不含任何缓存逻辑。
4. **声明的要求与安全模式**：Session 的配置可以声明要求，例如默认的 Agent 配置要求救生集常驻（`fs_*`、`shell_exec`、`plugin_status/deploy/rollback`、`manual_read`、`capability_search`）。内核据此校验计划，不满足的计划按 composer 失败处理：回退代际或进入安全模式。安全模式下的出厂 composer 永远带有救生集。角色扮演一类的 Session 可以不声明这项要求。

**缓存优化的分工。** composer 知道哪些内容是稳定的（这是语义），所以在片段上标注 `stable` 或 `volatile`。默认 composer 在一个请求系列内只追加披露，在压缩时开始新系列。Provider 知道怎样在线上利用稳定性（例如在稳定边界放置缓存断点、用原生的延迟加载保持工具列表不变）。内核记录命中率。换一个 composer 或 Provider，正确性都不受影响。

**SillyTavern 式组装。** 它是一个 composer 实现（prompt manager 的顺序、深度注入、宏展开，都是它内部的插槽模型），再加上若干 Context 贡献（例如按关键词扫描最近消息的世界书、作者注释）。内核不定义任何插槽，插槽是 composer 自己的词汇。Context 贡献描述候选块时使用一小组共享的种类（instruction / knowledge / memory / reminder / example / state），再加上由具体 composer 自行解释的不透明元数据。每个候选还说明它是常驻的（不看最新输入也会提供），还是因最新输入而召回的：前者可以留在请求开头，后者跟着它所回应的消息，这样请求可以只追加。这与系统其他地方使用的两层契约相同：控制信息有类型，业务内容是受 schema 约束的 JSON。`round.compose` 是"每个 Session 一个"，而不是全局单例：Agent 会话使用默认 composer，角色扮演会话使用 SillyTavern 式 composer，两者可以并存。文本补全（instruct 模板）格式化属于 composer 与 Provider 之间的约定，通过 Provider 的扩展字段表达，目前不需要。

所有可披露的条目使用同一个三级模型：

| 级别 | 内容 | 例子 |
|---|---|---|
| L0 目录 | 名称 + 一句话 | 工具、Skill、插件、远程节点的能力 |
| L1 说明 | 完整描述与 schema | 工具 schema、SKILL.md 正文、插件手册 |
| L2 参考 | 细节与引用，按需读取 | 长结果文件、Skill 附件、Brief 的引用、CONTRACT 的某个小节 |

这是 Agent Skills 三级结构在所有条目上的推广。已有的长结果落盘和 Brief 引用都是 L2 的实例。

**脚本调用。** 披露决定模型要读多少定义，脚本决定它要读多少中间结果（做法参照 pi 的 codemode）。

- 模型可以把一组工具调用写成一段脚本。脚本在宿主的沙箱中执行（例如编译成 Wasm 的 QuickJS），唯一的能力是把调用交回本 Round 的分派。
- 每个嵌套调用照常检查披露、经过 `tool.gate`、按 `outcome` 结算，记入 Log 时注明所属的脚本调用；结果只交给脚本，模型只看到脚本的输出。脚本内的搜索也是一次嵌套的 `capability_search`，搜索返回的能力因记录而成为已披露的（机制 1）。
- 脚本调用的 effect 为 `side-effect`，中断后记为 `unknown`，不重跑。脚本没有正常结束时（失败或中断），结算内容列出已开始的嵌套副作用调用及其结局，模型据此不重复它们。
- 一个能力直接声明给模型，还是只在脚本中可调用，是 composer 的披露选择，两者都记入披露集。

## 7.2 一份文档模型，两种读者

L0/L1/L2 的文本不单独维护，而是从源码文档中提取：

- 第一段 → L0 摘要（rustdoc 的惯例）
- 正文 → L1
- 带标题的小节与链接 → L2

同一段 `///` 既生成开发手册，也生成运行时的披露文本。只面向开发者的内容写在 `# Implementation` 小节里，提取给模型时剔除。这是 deepseek-harness "What the model sees" 的自动化版本：模型看到什么，由生成器从同一来源得出，并附上 token 估算写进 README，不再手写。

## 7.3 从 WIT 生成 CONTRACT.md

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

## 7.4 手册的层级与生成物

| 层 | 内容 | 来源 | 位置 |
|---|---|---|---|
| L0 | 常驻指令 + 地图（去哪里找什么） | 手写，限字数 | `AGENTS.md` |
| L1 | 宿主与插件的契约 | WIT 生成 | `wit/CONTRACT.md`、`wit/CONTRACT-CHANGES.md` |
| L1 | 目录：工具、事件、配置项、能力需求、原生例外 | 代码生成 | `docs/generated/*.md` |
| L1 | 插件手册：导出的接口与工具表（effect、摘要、token 估算）、所需导入、配置项 | 构建元数据 + 手写小节 | `plugins/<name>/README.md` 的生成区 |
| L2 | 内核与 SDK 的公开 API 索引（条目、签名、摘要、`file:line`） | rustdoc JSON | `crates/<crate>/API.md` |
| L3 | 源码 | — | — |

- 公开 API 索引来自 rustdoc JSON，使用仓库固定的同一个 stable 工具链生成，不引入第二个工具链：`RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-Z unstable-options --output-format json" cargo doc --no-deps`（已在 rustc 1.98.1 上实测，输出 `format_version` 60，保留文档注释）。`RUSTC_BOOTSTRAP` 只在 xtask 的这一步中设置，并使用独立的 target 目录。JSON 格式随工具链版本变化，所以 xtask 使用与之匹配的 `rustdoc-types` 版本，工具链升级时一起更新。
- **API.md 本身就是公开 API 的快照**：它提交入库，kernel 或 SDK 的公开 API 一旦变化，就会以 API.md 的 diff 出现在评审中，`--check` 保证它不过期。不需要再引入 `cargo public-api`：所有文档，包括 API 差异，都走同一个"生成物 + `--check`"机制。
- HTML 版 rustdoc 照常生成，给人看。Agent 读 Markdown。

## 7.5 两种手册：源码手册与运行时手册

- **源码手册**描述 git 中的版本。WIT 契约和内核、SDK 的 API.md 由 `cargo xtask docs` 生成；插件手册（README 生成区）是以空配置调用 `describe()` / `list-tools()` 得到的手册。
- **运行时手册**描述正在运行的代际：从制品中解码嵌入的 WIT，加上以该代际的配置调用 `describe()` / `list-tools()` 的结果，以代际为键存储。配置不同，手册就可能不同（§4.5）；其中只取决于制品的部分（解码出的 WIT）可以按制品哈希缓存。
- 插件手册只有一个生成器，位于 `enco-wasm` 的构建与校验中，输入是制品和一份配置：`plugin_build` 用空配置生成 README 生成区，`plugin_deploy` 用新代际的配置生成运行时手册；本仓库的出厂插件由 xtask 调用同一个生成器。
- CLI `enco manual <contract|plugin|capability> [--node X]` 和给运行中的 Agent 使用的工具 `manual_read`，读取的都是运行时手册。所以手册永远与实际执行的代码一致，并且按节点区分，因为不同节点可能运行不同的代际。
- 手册描述代际是什么，随代际固定不变；代际现在是否健康、接到了谁、在哪些节点上准入，属于 `plugin_status`（§4.10）。

## 7.6 自动化流程与门禁

```text
cargo xtask docs            生成全部生成物
cargo xtask docs --check    CI / pre-commit：WIT lint、生成物是否过期、API 差异、字数与 token 预算
plugin_build                构建 → clippy（仓库的 lint 表）→ 解码 WIT 与元数据 → lint（文档完整性、描述长度）
                            → 以空配置更新插件 README 生成区（git diff 对 Agent 可见）
plugin_deploy               以新代际的配置生成运行时手册，随代际一起生效
```

`plugin_build` 使用的 lint 表与本仓库根 `Cargo.toml` 的 `[workspace.lints]` 相同，由构建流程传给 clippy，所以修改插件自己的 `Cargo.toml` 放宽不了它。仓库里的出厂插件与 Agent 在 `~/.enco/plugins/` 中维护的插件因此遵守同一套规则。

生成区用 `<!-- generated:begin -->` / `<!-- generated:end -->` 标记，手工修改生成区会被 `--check` 拦下。**文档过期在这里是构建失败，而不是一个靠自觉维持的习惯。**
