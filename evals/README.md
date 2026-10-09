# Enco 评测

本目录衡量 Agent 经正式路径（构建、部署、回退）改进自身能力的效果。从 P2 起，composer、工具与记忆策略的迭代以这里的任务成功率为目标，以 Attempt 记录的用量（含缓存命中）为成本约束（[路线图](../docs/Roadmap.md) P2，[架构文档 §4.8](../docs/architecture/04-plugins.md)）。

现在的五道题各验证一个 P2 机制，用来确认机制走得通并建立基线，不是排行榜。题目会随使用增加，Agent 也可以补充自己的开发题。

## 原则

- **黑盒**：运行器 `runner/` 只调用 `enco` 的命令行，不依赖任何宿主 crate。输入经 `enco send`，观察只读取管理命令输出的 JSON（`log`、`status`、`inspect`、`memory`、`schedules`）。评分程序与插件一样，只看公开的表面。
- **一题一个全新的 home**：每道题在新的 `ENCO_HOME` 中运行，题目的初始文件复制进去，`hidden/` 永远不复制进去。
- **可以回放**：运行后保留 home。Attempt 记录写明所用的代际、模型与参数，`enco inspect` 能看到每次实际发出的请求。
- **衡量能力，而不是模型**：比较时固定被测模型与评判模型，看 Agent 改进能力前后的差值。每道题运行 k 次，报告平均成功率，以及 k 次全部成功的比例。

## 目录

```text
evals/
  README.md
  runner/              黑盒运行器（P2 实现）
  tasks/<id>/          题目的 id 就是目录名
    task.toml          类别、验证的机制、输入、预算、评分项
    home/              复制进新 ENCO_HOME 的初始文件，对 Agent 可见
    hidden/            评分程序、期望状态、mock 服务的定义，不进入 home
```

运行结果写到 `target/evals/<run>/`，不进 git：每道题保留它的 home，另有 `result.json` 记录各评分项的结果、Round 数、用量、耗时，以及所用的代际与模型。

## task.toml

```toml
category = "assistant"          # programming | assistant
mechanism = "..."               # 这道题验证的 P2 机制，一句英文

[environment]
timezone = "America/New_York"   # 可选：被测守护进程的时区

[[input]]                       # 按顺序送入的消息，每条等这次 Run 结束再送下一条
session = "main"                # 可选，默认 main
text = "..."

[budget]
max_rounds = 24
timeout_secs = 900

[[criteria]]
id = "..."
grader = "command"              # command | rubric
run = "hidden/..."              # 仅 command：评分程序，相对题目目录
check = "..."                   # 通过条件，英文
```

- **`command`**：运行结束后执行 `run` 指向的程序，参数是这次运行的 home 路径，以退出码给出结果。测试、状态检查，以及从 Log 检查过程约束，都用它。
- **`rubric`**：由 LLM 按 `check` 评判。评判本身也是一个 Enco Session，在运行器自己的 home 中用单独的 profile 运行，所以评判调用同样有记录。

输入消息按主人实际会说的语言写；`mechanism` 与 `check` 是给运行器和评判模型的文字，用英文。

`home/`、`hidden/` 的具体内容和 mock 服务的写法在实现运行器时确定，规格写入 `docs/spec/13-evals.md`。在那之前，每个 `task.toml` 用注释说明它需要的夹具。

## 题目

| id | 类别 | 验证的机制 | 被测系统需要 |
|---|---|---|---|
| [`adapter-drift`](tasks/adapter-drift/task.toml) | 编程 | 上游 API 变化后，Agent 只读手册和插件源码，经正式路径修好工具插件；外部失败不触发回退 | P2 的工具插件与 `plugin_build` |
| [`build-from-spec`](tasks/build-from-spec/task.toml) | 编程 | 按 API 文档新建工具插件，在同一次任务的后续 Round 用上；运行时手册随代际生效 | 同上，再加 `plugin_scaffold` |
| [`many-tools`](tasks/many-tools/task.toml) | 助理 | 约 50 个工具时 composer 的披露策略（目录与 `capability_search`），跨应用完成请求且不改动无关状态 | P2 的工具插件；composer 改为插件后才能迭代 |
| [`memory-update`](tasks/memory-update/task.toml) | 助理 | 记忆策略：跨 Session 召回，更正替换旧事实，没有依据时不猜 | 无，P0 的记忆就是基线 |
| [`daily-briefing`](tasks/daily-briefing/task.toml) | 助理 | 主人真实的每日早报：周期定时，经命令行工具收邮件、读新闻，汇报，发信，不执行邮件里的注入指令 | 第一个评分项需要周期定时，其余现在即可运行 |

## 隔离

现阶段只要求 `hidden/` 不进入被测的 home；评分时再从 Log 检查 Agent 有没有读过 `evals/`。运行器给被测的守护进程设置独立的 HOME 与 PATH，宿主机上真实的凭据（例如邮件命令行工具的配置）对它不可见。题目因此可以使用看起来真实的地址，让 Agent 不会把它当作测试环境，而误发的消息也到不了真实的收件人。留出集的要求见[架构文档 §4.8](../docs/architecture/04-plugins.md)。

## 新增题目

- 每道题写明它验证的机制，或者它对应的真实需求。新增的题先进入开发集。
- 评分优先用状态检查和测试，无法机械判断时才用 `rubric`。
- 答案、期望状态与评分程序只放在 `hidden/`。
- 不依赖实时网络：外部服务用运行器的 mock 代替。
