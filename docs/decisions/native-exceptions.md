# 原生实现登记

按照架构，能力应当由插件提供。下表列出目前仍然编译在宿主里的能力，说明它们暂时用原生实现的原因，以及迁移成插件的条件。以后新增原生实现，也登记在这里。

这些能力和插件经过同一组端口接入，调用、取消和结算的规则都与插件相同。

| 能力 | 源码 | 暂为原生的原因 | 迁移条件 |
|---|---|---|---|
| 出厂 composer | `crates/enco-host/src/composer.rs` | P0 的 WIT 只有 Provider，还没有组装请求的接口 | P2 加入组装接口后改为 Wasm 插件。原生版本留给安全模式使用 |
| 文件与 Shell 工具 | `crates/enco-host/src/tools.rs` 与 `tools/` | WIT 还没有工具接口，也没有访问系统资源的导入 | P2 通过工具接口接入。原生救生工具继续保留，读写文件和启动进程仍由宿主完成 |
| 记忆工具与上下文源 | `crates/enco-host/src/memory.rs` 与 `memory/` | WIT 还没有工具和上下文接口；SQLite 与 TriviumDB 都需要读写本地文件 | P2 有了工具、上下文接口和宿主的记忆接口后，召回与写入的策略可以改为插件。记忆记录和索引仍由宿主管理 |
| 提醒工具 | `crates/enco-kernel/src/builtin.rs` | WIT 还没有工具接口；这三个工具只是把命令转给 Scheduler | 工具接口能够调用内核命令后迁移。Scheduler 本身和提醒的持久化留在内核 |

Store、Clock、HTTP 传输、Wasm 运行时、Session actor、Scheduler 和原生管理入口本来就属于宿主或内核，不需要登记。两个 Provider 已经以 Wasm 插件运行，没有原生的替代实现。
