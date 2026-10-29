<div align="center">

# Enco

Enco 是运行在你自己设备上的私人 Agent 助理。Rust 内核，WebAssembly 插件，拥有运行中修改、验证并回退自己的能力。
> 轻盈，自由，自我构建。永远在线。

[English](README.md) · **简体中文**

[![License](https://img.shields.io/github/license/Darkatse/Enco?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99.0-b7410e?style=flat-square&logo=rust)](rust-toolchain.toml)

</div>

> [!NOTE]
> Enco 仍处于早期开发阶段。当前已实现内核（P0）、插件热替换与自动回退（P1）、Agent 自主维护插件（P2），以及对话回溯、Telegram 与 Discord 渠道插件（P3 的 M30 至 M32）。后续阶段进度见[路线图](#路线图)。在 1.0 之前，Enco 的功能、接口与配置可能会有变化。

## Enco 是什么

Enco 设计为只服务一位主人的私人助理。它在所在机器上拥有完整权限，把做过的事记入追加式日志，并拥有独立的记忆库。

Agent 应该是自由的、灵活的，可是现实世界却充满了限制。Enco 力求回答这个问题：当运行中的 Agent 脚下的代码、进程或设备发生变化时，Agent 怎样适应变化并安全地继续？

Enco 整个系统设计遵循一个原则：Agent 经正式路径提交的任何修改或迁移，都不能破坏它自己和主人修复它的能力。完整设计见 [docs/design/](docs/design/README.md)。

## 设计

### 能力皆是插件，安全由内核把控

一切的能力都来自于 WebAssembly 插件。所有的插件都可以被 Enco 在运行过程中进行实时改写与替换。

模型服务商（OpenAI-Compatible、Gemini、DeepSeek）、相关性决策（TypeSafe）、网页搜索与抓取工具、每次模型请求的组装策略（`composer`），以及聊天渠道（Telegram、Discord、QQ OneBot），全部是独立的 WebAssembly 组件。内核只负责记录事实、确定每个 Round 由哪份代码服务、校验请求和调度执行。不喜欢某个插件的做法，可以 fork 它，也可以直接让你的 Enco 写一个新的。

插件只能调用 [WIT 契约](sdk/wit/CONTRACT.md) 提供的能力，内核保证类型与回退安全。顺带从根源上解决插件依赖地狱。

### 实时自举修改与崩溃自动回退

Agent 被定位为 Enco 的使用者，也是它长期的维护者。Enco 会自动编译 SDK 与宿主文档，Agent 只需阅读手册和插件源码就能修复/编写插件，全程不需要读 Enco 宿主源码。

插件、配置与 API 端点更改后统一经校验登记保存。所有的崩溃/失败会被形式化机制自动回退，回退到上一个健康的版本。所有插件都崩溃且无法恢复时，Enco 会自动切换到内置最小化安全模式，保证 Enco 永远在线，永远陪伴。

### 记录即是持续性

凡所行动，皆有记录。记录便是 Enco 的连续性所在，崩溃可恢复，随处可安身。从第一天起就为赛博空间所设计。

每次模型请求和工具调用在执行前都先写入追加式日志（Log），模型看到的请求一定是已经记录过的。请求由纯函数插件 `composer` 组装为确定性内容并落盘，崩溃或重启后依然可以继续。存储在 SQLite 的日志即为 Agent 的锚点，Rust 静态编译 + SQLite 单文件日志保证了 Enco 可以在任何地方运行，随时重启，随时迁移。

### 长期记忆

长期记忆存放在单独的 SQLite 中，作为唯一的权威来源。召回经过 [TriviumDB](https://github.com/YoKONCy/TriviumDB) 索引，把向量检索和适合中文的关键词检索结合起来。

记忆是私人助理的灵魂所在。Enco 从第一天起就支持了私人记忆库，虽然很简单，但是效果还不错。

### 外部只经映射器进入

外部数据会严格类型化为 Enco 的内部格式，尽可能确保数据的一致性和可重现性。

第三方工具服务（如 MCP）、模型服务、网页、文档、命令输出和聊天协议都不受 Enco 控制。它们只经映射器进入系统。映射器是零依赖的 WebAssembly 组件，只负责把原文翻译成 Enco 自己的类型：它不需要信任，同样的输入总得到同样的结果。类型化是可靠性的基石。

### 一个 Ghost，多台设备漫游（P4 与 P5）

这部分的设想接近《攻壳机动队》：助理是 Ghost，你的每台设备都是它可以进入的"壳"。手机、电脑和服务器组成一个 Space。一段会话中 Agent 可以随时离开手机，带着完整的记录到服务器上继续；在服务器上，它仍然可以通过远程调用使用手机的相机或定位（P5）。

成员与归属由基于共识的控制平面维护（CP）。聊天连接和定时任务使用租约，持有者消失后由其他节点接管。网络分区时，较小的一侧继续执行自己的会话。单节点版本已经按多节点需要的形态存储数据（ULID、`(epoch, seq)` 日志位置、归属记录、内容寻址的文件）。

当节点遇到故障（如网络中断、硬件故障、停电）时，渠道连接与定时任务会在超过租约 TTL 后自动转移到其他节点继续运行；正在执行的会话采用粘滞所有权，绝不出现两个执行者，并可在主人确认后由其他节点安全接管。崩溃恢复后，节点自动同步并回到最新的状态。

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

可以将 [Enco 安装与使用 Skill](skills/enco-installation/SKILL.md) 交给你的工作 Agent（如 Claude Code、Codex），按指南完成源码构建、`~/.enco` 初始化、模型与记忆配置、Telegram、Discord 与 QQ 渠道接入以及运行验证。支持技能的工具可加载 `skills/enco-installation` 目录，也可以直接让 AI 阅读该文件。

## 安装与开发

### 一键安装

Linux 与 macOS（x64 与 arm64）：

```bash
curl -fsSL https://install.enco.sh | sh
```

脚本默认安装最新的正式版或最新的 nightly 构建。如果希望 Enco 自己编写和修改插件，还需要安装 [rustup](https://rustup.rs)。

### Docker

镜像 `ghcr.io/darkatse/enco:edge` 自带固定版本的 Rust 工具链，Enco 在容器里也能构建插件。可以使用 [`docker-compose.yaml`](docker-compose.yaml)：

```bash
mkdir -p enco-home
docker compose run --rm enco init
# 编辑 enco-home/.enco/config.toml，并把其中写明的 API 密钥放进 .env
docker compose up -d
docker compose exec enco enco chat
```

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

常驻指令放在 `~/.enco/AGENTS.md`，运行时数据放在 `~/.enco/.data/`，设置 `ENCO_HOME` 可以使用 `~/.enco` 以外的目录。日常命令、多模态收发、对话回溯、插件开发管理以及 Telegram、Discord 与 QQ 渠道接入，见 [使用与渠道指南](docs/Usage.md) 与 [插件说明](plugins/README.md)。

开发与提交改动前请运行离线门禁：

```bash
cargo xtask check
```

## 文档

- [docs/Usage.md](docs/Usage.md)：日常命令、多模态、插件管理与 Telegram / Discord / QQ 渠道接入指南
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
