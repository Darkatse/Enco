<div align="center">

# Enco

运行在你自己设备上的私人 Agent。它用 Rust 编写，所有功能都由 WebAssembly 插件提供，Agent 可以自己维护和替换这些插件。

[English](README.md) · **简体中文**

[![License](https://img.shields.io/github/license/Darkatse/Enco?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.98.1-b7410e?style=flat-square&logo=rust)](rust-toolchain.toml)

</div>

> [!NOTE]
> Enco 处于早期开发阶段。内核（P0）已实现，并通过测试与审核。标有阶段（例如 P1）的小节描述的是已经设计、尚未实现的部分，见[路线图](#路线图)。

## Enco 是什么

Enco 是只服务一位主人的助理。它在所在的机器上拥有完整权限，把自己做过的事记录在追加式日志里，并且跨会话记住你告诉它的事。内核很小，功能都放在插件里，Agent 可以自己阅读、修复并重新构建这些插件。

完整设计见 [docs/Architecture.md](docs/Architecture.md)。

## 设计

### 一切都是插件

模型服务商、聊天渠道、工具，以及组装每次模型请求的策略，都是插件。内核只负责记录、确定每个 Round 由哪份代码服务、校验请求和调度，所以任何功能都可以替换。不喜欢某个插件的做法，可以 fork 它，也可以让你的 Enco 自己写一个新的。

插件是 WebAssembly 组件，只能使用 WIT 契约提供的能力。宿主不按插件的身份写分支，插件之间只能经由内核互相调用。每次调用使用新的实例，插件内部崩溃只会让这一次调用失败，不影响其他部分。

P0 中模型服务商已经以插件形式运行；工具和请求组装在 P2 改为 WebAssembly 插件，它们的内置版本保留下来，作为安全模式使用的兜底实现。

### 为 Agent 设计

Agent 是 Enco 的首要使用者，也是它长期的维护者。能力、源码位置和最近的失败应当容易被它找到，修改一种能力只需要读懂一个接口。插件文档（`wit/CONTRACT.md`）由 WIT 契约生成，生成物过期时 `cargo xtask check` 会失败。

### 热更新与回滚（P1）

Agent 可以在 Enco 运行时修改插件、重新构建并替换上线。新代码从下一个 Round 开始生效，Round 边界是唯一替换代码的时机。新版本通过健康检查才会保留，否则退回上一版。如果改坏得比较严重，安全模式会保留一小组文件、Shell 和部署工具用来修复。替换宿主程序本身始终需要主人确认。

### 可靠的记录

每次模型请求和工具调用在执行前都先写入追加式日志，模型看到的请求一定是已经记录过的。进程在任务中途被杀掉，重启后被打断的 Round 会明确标记为中断。无法确认结果的工具调用记为 `unknown`，不会自动重试。

### 记忆

长期记忆以记录的形式存放在 SQLite 中。召回经过 [TriviumDB](https://github.com/YoKONCy/TriviumDB) 索引，它把向量检索和适合中文的关键词检索结合起来。索引随时可以删除并从记录重建；每条检索结果都要回到记录中重新读取，所以已经更正或遗忘的记忆不会因为索引过期而重新出现。置顶记忆优先进入上下文。embedding 服务不可用时，召回退回到关键词检索，并在记录的请求中注明原因。

### 多设备漫游（P4）

这部分的设想接近《攻壳机动队》：助理是 Ghost，你的每台设备都是它可以进入的"壳"。手机、电脑和服务器组成一个 Space。一段会话可以在两个 Round 之间离开手机，带着完整的记录到服务器上继续；在服务器上，它仍然可以通过远程调用使用手机的相机或定位（P5）。和 Ghost 一样，它同一时刻只在一个壳里：一段会话不会同时在两台设备上运行。

### 一致性优先（P4）

成员与归属由基于共识的控制平面维护。一段会话任何时候只有一个归属者，不会同时在两台设备上运行。聊天连接和定时任务使用租约，持有者消失后由其他节点接管。网络分区时，较小的一侧可以继续自己的会话，但不会接管别处的会话。单节点版本已经按多节点需要的形态存储数据（ULID、`(epoch, seq)` 日志位置、归属记录、内容寻址的文件）。

### Rust

内核运行在 tokio 上，插件运行在 Wasmtime 上。在桌面原型上，插件增量构建约 1 秒，编译组件约 27 毫秒，创建实例约 40 微秒，一次调用约 1.5 微秒。和模型的延迟相比插件开销很小，所以性能工作主要放在减少大块数据的复制上。

### 平台

P0 在 macOS 和 Linux 上以后台服务加命令行客户端的形式运行，目前已在 macOS 上测试。手机之后作为完整节点加入（P5）：Android 用 Cranelift 运行插件，iOS 用 Pulley 解释器。系统挂起应用之前，手机会把手上的会话交给常驻节点。Windows 暂不支持，因为本地协议使用 Unix domain socket。

### 少量一致的概念

整个系统用十个概念和两条规则来描述可变状态：每份状态只有一个归属者，由它按顺序处理变更；回调把写入作为返回值交出，由归属者在一个事务中提交。聊天连接、有状态的工具和派生索引都遵循这两条规则，没有各自的一套框架。实施规格固定了术语，并列出了代码中应当避免的结构。

## 路线图

| 阶段 | 目标 | 状态 |
|---|---|---|
| P0 | 内核：会话、持久日志、崩溃恢复、OpenAI-Compatible 与 DeepSeek Provider 插件、记忆、提醒、安全模式 | 已完成 |
| P1 | 运行时替换插件，带健康检查与回滚 | 计划中 |
| P2 | Agent 只读手册和插件源码，自己维护插件 | 计划中 |
| P3 | 聊天渠道：Telegram、QQ（OneBot） | 计划中 |
| P4 | 多设备：控制平面、会话交接、复制 | 计划中 |
| P5 | 手机作为节点 | 计划中 |

## 从源码构建

需要 Rust 1.98.1 与 `wasm32-wasip2` 目标。版本固定在 `rust-toolchain.toml` 中，第一次构建时 `rustup` 会自动安装。

```bash
git clone https://github.com/Darkatse/Enco.git
cd Enco
cargo xtask build-factory      # 构建内置的 Provider 插件
cargo run -p enco -- init      # 创建 ~/.enco 与配置模板
```

编辑 `~/.enco/config.toml`（示例见 [`examples/`](examples/)），导出其中指定的 API key 环境变量，然后：

```bash
cargo run -p enco -- serve     # 启动服务
cargo run -p enco -- chat      # 在另一个终端中
```

设置 `ENCO_HOME` 可以使用 `~/.enco` 以外的目录。提交改动前请运行 `cargo xtask check`。

## 文档

- [docs/Architecture.md](docs/Architecture.md)：架构文档
- [docs/P0/](docs/P0/)：P0 内核规格，描述已实现的系统
- [wit/CONTRACT.md](wit/CONTRACT.md)：插件契约，由 WIT 生成
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

整体设计还借鉴了四个更早的系统。Erlang 提供了进程持有自身状态、在监督下替换代码的思路。Plan 9 的进程命名空间与 `import`、`cpu` 命令，对应到每个 Round 的能力快照、`invoke` 与 `handoff`。Smalltalk 展示了一个在运行中修改自身的系统。Lisp 把代码当作数据，Enco 也把日志和每一次模型请求当作数据。

Enco 基于 [Wasmtime](https://wasmtime.dev/)、[tokio](https://tokio.rs/) 与 [SQLite](https://sqlite.org/) 构建。记忆召回使用 [@YoKONCy](https://github.com/YoKONCy) 的 [TriviumDB](https://github.com/YoKONCy/TriviumDB)。

## 许可

以 [Apache License 2.0](LICENSE) 发布。
