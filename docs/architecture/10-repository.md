# 10. 仓库结构

```text
crates/
  enco-core/     # 类型：Event、Message、Outcome、NodeId、Epoch；无 IO
  enco-kernel/   # Session actor 与监督树、Round、ContextPlan 校验与记录、能力快照、代际、Binding、Approval；不依赖 wasmtime 与网络
  enco-host/     # Host 服务：SQLite、blob、fs、exec、检索索引与记忆、http/ws 连接管理、调度
  enco-wasm/     # wasmtime 嵌入、WIT 绑定、组件适配、构建与校验
  enco-space/    # 节点身份与配对、Peer 连接、控制平面（openraft）、日志复制、invoke/handoff
  enco-sdk/      # 插件 SDK：绑定封装、#[tool] 宏、host fake
apps/
  enco/          # 单一二进制：serve / witness / chat / plugin * / space * / status；组合根；原生管理通道
  mobile/        # 之后再做
xtask/           # cargo xtask docs [--check]：WIT → CONTRACT、目录、API 索引、出厂插件的 README 生成区（与 plugin_build 共用生成器）、lint（§7）
plugins/
wit/             # *.wit + 生成的 CONTRACT.md / CONTRACT-CHANGES.md
docs/{AGENTS.md, decisions/, cookbook/, generated/}
scripts/check-crate-boundaries   # 依赖方向（§4.1）
```

依赖方向：`enco-core` ← `enco-kernel` ← `enco-host` / `enco-wasm` / `enco-space` ← `apps/enco`。内核只知道 `NodeId`、`Binding`、`ControlPlane` 和"把调用或事件交给某个节点"这几个 trait。

**crate 按概念划分，按触发条件拆分。** Enco 已经满足 Clean Architecture 的依赖规则：`enco-core` 是实体，内核端口与 WIT 是端口，内核是用例，`enco-host`、`enco-wasm` 与 Wasm 插件是适配器，`apps/enco` 是组合根；最外层的插件由 Wasm 物理隔离。因此不另建 domain / contracts / ports / application / adapter 这类通用分层 crate，也不引入 Repository、Service、DTO 这套与本文词汇并行的命名。crate 的名字对应 §3.2 的概念；只有出现明确的边界、独立的依赖成本或稳定的变化原因时才拆分，不为"看起来分层"而拆。

已知的第一个拆分点是 `enco-host`。它同时承载两个概念：**Host 机制**（Store、blob、KV、fs/exec/http 原语、检索索引、时钟）与**原生能力**（出厂 composer、救生工具、工作区上下文、记忆策略）。

- **触发条件**：WIT 第一次需要 `log`、`http` 之外的宿主导入（例如 `state-get`、`blob-put`、`exec`，预计在 P1–P2）。那时插件运行时必须调用 Host 机制，而当前的依赖方向不允许 `enco-wasm` 依赖 `enco-host`。
- **拆分后的形态**（名称到时再定）：

  ```text
  enco-core ← enco-kernel ← enco-host     Host 机制
                           ← enco-wasm     插件运行时：经 WIT 把 Host 机制交给插件（依赖 enco-host）
                           ← enco-native   原生能力（依赖 enco-host）
                           ← apps/enco
  ```

- **触发之前**保持模块级分隔，不预先拆分：提前拆分会迫使记忆的检索索引变成只有一个使用者的通用机制。

Agent 可读性约定：`AGENTS.md` 只写常驻指令和地图；README 中的 What the model sees / Token effect 由生成器产出（§7.2）；所有目录都是生成物，并设置 `--check` 门禁；决策记录放在 `docs/decisions/`；维护一张失败策略表，和一张"需要重启或主人确认的改动"表。
