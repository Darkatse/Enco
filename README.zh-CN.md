<div align="center">

# Enco

运行在你自己设备上的私人 Agent。Rust 内核，WebAssembly 插件边界，能够在运行中修改、验证并回退自己的能力。

[English](README.md) · **简体中文**

[![License](https://img.shields.io/github/license/Darkatse/Enco?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99.0-b7410e?style=flat-square&logo=rust)](rust-toolchain.toml)

</div>

> [!NOTE]
> Enco 仍处于早期开发阶段。内核（P0）、插件热替换与自动回退（P1）、Agent 自主维护插件（P2），以及对话回溯、Telegram 与 Discord 渠道插件（P3 的 M30 至 M32）均已实现并通过测试验收。后续阶段标注在各小节标题中，进度见[路线图](#路线图)。

## Enco 是什么

Enco 是只服务一位主人的私人助理。它在所在机器上拥有完整权限，把做过的每件事记入追加式日志，并跨会话记住你告诉它的事。

私人助理最容易被两件事打断：想加一项顺手的能力，得停下来等开发者改代码；换台设备、重启进程或者改坏了一个模块，正在做的事就断了。Enco 把这两件事当成同一个问题处理：当运行中的 Agent 脚下的代码、进程或设备发生变化时，手头的工作怎样安全地继续？

整个系统守住一条底线：Agent 经正式路径提交的任何修改或迁移，都不能破坏它自己和主人修复它的能力。完整设计见 [docs/design/](docs/design/README.md)。

## 设计

### 内核只管事实，能力全部来自插件

模型服务商（OpenAI-Compatible、Gemini、DeepSeek）、相关性决策（TypeSafe）、网页搜索与抓取工具、每次模型请求的组装策略（`composer`），以及聊天渠道（Telegram、Discord），全部是独立的 WebAssembly 组件。内核只负责记录事实、确定每个 Round 由哪份代码服务、校验请求和调度执行。不喜欢某个插件的做法，可以 fork 它，也可以直接让你的 Enco 写一个新的。

插件只能调用 [WIT 契约](sdk/wit/CONTRACT.md) 提供的能力。宿主从不按插件身份写业务分支，插件之间只经内核转发调用。每次调用都在全新的 Wasm 实例中进行，插件内部崩溃只会让当次调用失败，不会波及其他部分。插件也永远拿不到密钥明文：配置里只写环境变量名，由宿主在传输层按绑定的目标域名写入请求，并在响应返回给插件前自动脱敏。

### 运行中修改自己，改坏了能退回来

Agent 是 Enco 的首要使用者，也是它长期的维护者。能力、源码位置和最近的失败都可以直接查到，修改一种能力只需要读懂一个接口。宿主把插件契约（`sdk/wit/CONTRACT.md`）、SDK 接口与当前运行插件的代码结构实时生成为 `~/.enco/manual/` 下的只读手册，生成物过期时 `cargo xtask check` 会报错。Agent 只需阅读手册和插件源码就能修复或编写插件，全程不需要读宿主源码。

插件代码、插件设置与系统配置统一登记为代际（Generation），从下一个 Round 开始生效，Round 边界是唯一替换代码的时机。配置与接线在登记时就会发起一次最小真实调用，接不通的配置在生效前就会被拦下；新插件版本先进入试用，通过健康检查才会保留，崩溃或违反契约时自动退回上一版。如果一轮请求仍然失败，安全模式会换上内嵌的出厂插件与空选项，保留文件、Shell、登记与状态检视这组救生工具用来修复。替换宿主程序本身始终需要主人确认。

### 先记录，再发生

每次模型请求和工具调用在执行前都先写入追加式日志（Log），模型看到的请求一定是已经记录过的。请求由纯函数插件 `composer` 组装为内容寻址的计划并落盘，此后重试、查看（`enco inspect`）、崩溃恢复与跨机续接都直接解析这份记录，不再重新组装。

进程在任务中途被杀掉时，重启后被打断的 Round 会明确记为中断。工具调用的结局只有 `ok`、`failed` 和 `unknown` 三种：请求发出后无法确认结果的副作用调用一律记为 `unknown`，绝不自动重试，交由看得到日志的模型与主人定夺。

### 索引提名、记录裁决的长期记忆

长期记忆以记录的形式存放在 SQLite 中，作为唯一的权威来源。召回经过 [TriviumDB](https://github.com/YoKONCy/TriviumDB) 索引，把向量检索和适合中文的关键词检索结合起来。索引只是记录的派生物，随时可以删除并从记录重建；每条检索命中的结果都要回到 SQLite 重新读取，已经更正或遗忘的记忆不会因为索引过期而重新出现。置顶记忆优先进入上下文。embedding 服务不可用时，召回退回到关键词检索，并在记录的请求中注明原因。

可选的 TypeSafe 决策插件按相关性过滤自动召回的记忆，启用[配置示例](examples/deepseek-gemini.toml)中的 `[endpoint.relevance]` 并在 `[memory] relevance` 中选用即可。服务失败时保留召回结果并记录原因；置顶记忆和显式 `memory_search` 的结果不经过过滤。

### 外部只经映射器进入

第三方工具服务（如 MCP）、模型服务、网页、文档、命令输出和聊天协议都不受 Enco 控制。它们只经映射器进入系统。映射器是不带任何导入的 WebAssembly 组件，只负责把原文翻译成 Enco 自己的类型：它不需要信任，同样的输入总得到同样的结果。

原文在进入映射器前会先记录下来，新版本映射器可以在以前见过的全部内容上重放，与旧版本逐条比较。外部服务的契约（例如工具服务的工具目录）规范化后作为定本写进意图，与插件源码一样登记；服务一旦变化，差异会报告为漂移，不会悄悄改变模型看到的东西。Telegram 与 Discord 渠道已经按这一模式运行，MCP 与支持流式补全的模型服务商随后跟进：插件只做编码与解码，HTTP 与连接由宿主持有并只发往配置的 endpoint，原始响应留作重放。

### 一个 Ghost，多台设备（P4 与 P5）

这部分的设想接近《攻壳机动队》：助理是 Ghost，你的每台设备都是它可以进入的"壳"。手机、电脑和服务器组成一个 Space。一段会话可以在两个 Round 之间离开手机，带着完整的记录到服务器上继续；在服务器上，它仍然可以通过远程调用使用手机的相机或定位（P5）。和 Ghost 一样，它同一时刻只在一个壳里：一段会话不会同时在两台设备上运行。

成员与归属由基于共识的控制平面维护（CP）。聊天连接和定时任务使用租约，持有者消失后由其他节点接管。网络分区时，较小的一侧继续执行自己的会话，不会接管别处的会话。单节点版本已经按多节点需要的形态存储数据（ULID、`(epoch, seq)` 日志位置、归属记录、内容寻址的文件）。

### 少量概念与运行时开销

整个系统用十个概念和两条规则来管理可变状态：每份状态只有一个归属者，由它按顺序处理变更；回调把写入作为返回值交出，由归属者在一个事务中提交。聊天连接、有状态的工具和派生索引都遵循这两条规则，不各自另立框架。设计固定了术语，每个词只指一样东西。

内核运行在 tokio 上，插件运行在 Wasmtime 上。在桌面原型上，插件增量构建约 1 秒，编译组件约 27 毫秒，创建实例约 40 微秒，一次调用约 1.5 微秒。和模型的延迟相比，插件边界的开销很小，性能工作主要放在减少大块数据的复制上。

目前 Enco 在 macOS 和 Linux 上以后台服务加命令行客户端的形式运行（已在 macOS 上测试）。手机之后作为完整节点加入（P5）：Android 用 Cranelift 运行插件，iOS 用 Pulley 解释器，并在系统挂起应用前把手上的会话交给常驻节点。Windows 支持已列入路线图，Unix 环境优先，届时本地协议在 Windows 上改用命名管道代替 Unix domain socket。

## 路线图

| 阶段 | 目标 | 状态 |
|---|---|---|
| P0 | 内核：会话、持久日志、崩溃恢复、OpenAI-Compatible 与 DeepSeek Provider 插件、记忆、提醒、安全模式 | 已完成 |
| P1 | 运行时替换插件与配置，带健康检查与回滚 | 已完成 |
| P2 | Agent 只读手册和插件源码，自己维护插件并通过评测 | 已完成 |
| P3 | 渠道插件化：对话回溯（M30）、Telegram（M31）与 Discord（M32）映射器插件、接入 QQ OneBot（M33） | 进行中（M30 至 M32 已完成） |
| 监督树与脚本 | 把任务委派给可使用其他模型的子 Session；沙箱脚本编排工具调用 | 计划中 |
| P4 | 多设备：控制平面、Session 漫游、复制 | 计划中 |
| P5 | 手机作为节点 | 计划中 |

每个阶段的验收标准见 [docs/Roadmap.md](docs/Roadmap.md)。

## 使用 AI 协助安装

可以将 [Enco 安装与使用 Skill](skills/enco-installation/SKILL.md) 交给你的工作 Agent（如 Claude Code、Codex），按指南完成源码构建、`~/.enco` 初始化、模型与记忆配置、Telegram 与 Discord 渠道接入以及运行验证。支持技能的工具可加载 `skills/enco-installation` 目录，也可以直接让 AI 阅读该文件。

## 安装与开发

### 一键安装（计划中）

预编译二进制与一键安装脚本将在首个正式版本发布后提供；当前请从源码构建。

### 从源码构建

需要 Rust 1.99.0 与 `wasm32-wasip2` 目标。版本固定在 `rust-toolchain.toml` 中，第一次构建时 `rustup` 会自动安装。

```bash
git clone https://github.com/Darkatse/Enco.git
cd Enco
cargo fetch                   # 首次预取依赖；xtask 命令离线运行
for manifest in sdk/Cargo.toml evals/runner/Cargo.toml plugins/*/Cargo.toml; do
    cargo fetch --manifest-path "$manifest"
done
cargo xtask build-factory      # 构建内置插件
cargo run -p enco -- init      # 创建 ~/.enco 与配置模板
```

编辑 `~/.enco/config.toml`（示例见 [`examples/`](examples/)），导出其中指定的 API key 环境变量，然后启动服务：

```bash
cargo run -p enco -- serve     # 启动服务
cargo run -p enco -- chat      # 在另一个终端中
cargo run -p enco -- send --image photo.png "Describe this image"
```

常驻指令放在 `~/.enco/AGENTS.md`，运行时数据放在 `~/.enco/.data/`，设置 `ENCO_HOME` 可以使用 `~/.enco` 以外的目录。日常命令、多模态收发、对话回溯、插件开发管理以及 Telegram 与 Discord 渠道接入，见 [使用与渠道指南](docs/Usage.md) 与 [插件说明](plugins/README.md)。

开发与提交改动前请运行离线门禁：

```bash
cargo xtask check
```

## 文档

- [docs/Usage.md](docs/Usage.md)：日常命令、多模态、插件管理与 Telegram / Discord 渠道接入指南
- [skills/enco-installation/SKILL.md](skills/enco-installation/SKILL.md)：供工作 Agent 使用的安装与配置 Skill
- [docs/design/](docs/design/README.md)：设计总纲与各领域的判断
- [docs/Roadmap.md](docs/Roadmap.md)：各阶段的计划与验收标准
- [LESSONS.md](LESSONS.md)：实践中被证伪的做法，改代码前先读
- [sdk/wit/CONTRACT.md](sdk/wit/CONTRACT.md)：插件契约，由 WIT 生成
- [CONTRIBUTING.md](CONTRIBUTING.md)：项目理念与贡献方式

## 参与贡献

欢迎提交 Issue 和 PR。请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)，PR 的目标分支为 `main`。

## 致谢

Enco 的设计借鉴了这些项目：

- [Operit2](https://github.com/AAswordman/Operit2)：设备作为对等节点，工作可以在节点之间转移。
- [ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw)：直接基于 WIT 与 Wasmtime 的插件，以及按代际应用配置、忽略旧代际迟到结果的做法。
- [IronClaw](https://github.com/nearai/ironclaw)：由 Agent 自己构建的 WASI 组件工具，以及由宿主持有事件循环、每次渠道回调使用新实例的做法。
- [AstroBox](https://plugindoc.astrobox.online/)：同一套插件运行时覆盖桌面、Android（Cranelift）与 iOS（Pulley）。
- [OpenClaw](https://docs.openclaw.ai/nodes)：手机作为节点，向外提供设备能力。
- [Iris](https://github.com/Lianues/Iris)：插件的打包粒度，以及按逆序释放资源。
- [deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)：以 RAII 方式注册、单一 Inbox、检查模型看到的内容已被记录，以及 README 中的 "What the model sees"。
- [TauriTavern](https://github.com/Darkatse/TauriTavern)：工具调用 ID 的映射、参数归一化、把无法确认的结果记为 unknown，以及 [CONTRIBUTING.md](CONTRIBUTING.md) 中的工程原则。
- [SillyTavern](https://github.com/SillyTavern/SillyTavern)：因为它的 prompt manager，请求组装被设计成每段会话可以各自选择的插件。
- [Zed](https://zed.dev/)：同时链接旧版本 world 来演进 WIT 契约。

整体设计还通过五个透镜审视自己。Erlang 提供了进程持有自身状态、在监督下替换代码的思路。Plan 9 的进程命名空间与 `import`、`cpu` 命令，对应到每个 Round 的能力快照、`invoke` 与 `roam`。Smalltalk 展示了一个在运行中修改自身的系统。Lisp 把代码当作数据，Enco 也把日志和每一次模型请求当作数据。边界类型在边界上把外来数据一次翻译成语言自己的类型：F# 与 Idris 的类型提供器、Elm 的 decoder、带归责的 Typed Racket 类型化导入，以及 [TypeScript 的内容映射器](https://github.com/microsoft/typescript-go/pull/4712)。Enco 的映射器沿用这一思路，并补上它们的历史教训：映射器不能做 I/O，外部契约钉住而不是现查。

Enco 基于 [Wasmtime](https://wasmtime.dev/)、[tokio](https://tokio.rs/) 与 [SQLite](https://sqlite.org/) 构建。记忆召回使用 [@YoKONCy](https://github.com/YoKONCy) 的 [TriviumDB](https://github.com/YoKONCy/TriviumDB)。

## 许可

以 [Apache License 2.0](LICENSE) 发布。
