# Enco 评测

本目录衡量 Agent 经正式路径（构建、部署、回退）改进自身能力的效果。从 P2 起，composer、工具与记忆策略的迭代以这里的任务成功率为目标，以 Attempt 记录的用量（含缓存命中）为成本约束（[路线图](../docs/Roadmap.md) P2，[架构文档 §4.8](../docs/architecture/04-plugins.md)）。

题目各验证一个机制，用来确认机制走得通并建立基线，不是排行榜。题目会随 P2 的里程碑与使用增加，Agent 也可以补充自己的开发题。

## 运行

在仓库根目录构建被测程序与运行器；命令、配置与结果格式见 [spec 13](../docs/spec/13-evals.md)。

```sh
cargo xtask build-factory
cargo build -p enco
cargo build --manifest-path evals/runner/Cargo.toml
```

## 评测对进化的意义

Agent 改进自己的每一步，都要回答同一个问题：这次改动是不是更好。没有测量，改动只能凭感觉，改好与改坏无从分辨；有了评测，才有数据与指标，进化才有方向。评测因此是 Agent 自我进化的目标函数（[架构文档 §4.8](../docs/architecture/04-plugins.md)）：Agent 读取 `result.json`，按"目标题上更好、其余没有倒退"决定采纳还是拒绝一次改动，失败的原因又是它下一个假设的来源。

目标函数错了，进化会朝错的方向加速，而且越努力偏得越远：

- 评分放过了错误的结局，Agent 就会学会钻评分的空子，而不是学会做事；
- 评分拒绝了正确的结局，好的改动会被当成倒退丢掉；
- 题目有歧义，通过与否只是噪声，Agent 会追着噪声改；
- 题目一眼就能认出是测试，测到的是 Agent 应付考试的行为，而不是它平时为主人做事的样子，评测里的进步到了真实生活中就不复存在。

所以题目的两条要求排在一切之前：

- **有效性**：通过意味着这项能力真的可用，失败意味着它真的有问题。评分要检查的一切都在题目中说明，退化的做法通不过，合理的写法都能通过，评分本身用案例测试（见"题目自检"）。
- **真实性**：题目来自主人真实的需求或真实发生过的失败，Agent 看到的一切都像真实的生活：真实形态的数据、地址与人物，自然的干扰与噪声，没有测试的痕迹。只有这样，评测里的行为才等于 Agent 平时的行为，评测里的进步才是真实的进步。

宁可少一道题，也不要一道无效或失真的题：错误的信号比没有信号更糟。

## 原则

- **黑盒**：运行器 `runner/` 是独立的 Cargo 工作区，只调用 `enco` 的命令行，引用不了宿主的 crate。输入经 `enco send`，观察只读取管理命令输出的 JSON。评分与插件一样，只看公开的表面。
- **一题一个全新的 home**：每道题在新的 `ENCO_HOME` 中运行，题目的初始文件复制进去，`task.toml` 与 `hidden/` 永远不复制进去。
- **评分只读快照**：运行结束后，运行器经命令行把一切可观察的状态导出到快照目录。保存的评判回复让重评分可以离线进行，也可以用手写的快照测试评分本身。
- **可以回放**：运行后保留 home。Attempt 记录写明所用的代际、模型与参数，`enco inspect` 能看到每次实际发出的请求。
- **产出是比较，而不是分数**：被测模型与评判模型固定，改动前后的两个变体在同一时间窗口内交错运行同一批题，结论回答进化时的问题：目标题上是否更好，其余每道题是否没有倒退。每道题运行 k 次，报告通过次数，以及 k 次全部成功的比例；判定规则见 spec 13。

## 目录

```text
evals/
  README.md
  runner/                黑盒运行器，独立 Cargo 工作区
  tasks/<id>/            题目的 id 就是目录名
    task.toml            类别、验证的机制、输入、预算、评分项与参考答案
    home/                复制进新 ENCO_HOME 的初始文件，对 Agent 可见
    hidden/              不进入 home
      mocks/<名字>.toml  mock 服务
      schemas/           state 评分项的 JSON Schema
      cases/<名字>/      评分的测试案例
```

运行结果写到 `target/evals/<run>/`，不进 git：

```text
result.json
trials/<题>/<变体>/<i>/
  home/                      被测的 home
  snapshot/
    log/<session>.json       enco log
    tool-calls.json          由 Log 配对得出：每次工具调用的名字、参数与结局
    transcript/<session>.md  对话：按顺序的消息，每条以 "## owner" 或 "## assistant" 开头
    schedules.json  memory.json  plugins.json  status.json
    mock/<名字>/state.json   mock 的最终状态
    mock/<名字>/requests.jsonl
    network.jsonl            被代理拒绝的访问
  judge/<评分项>.json        评判的请求与回复
  grades.json
```

## task.toml

```toml
category = "assistant"          # programming | assistant
mechanism = "..."               # 这道题验证的 P2 机制，一句英文

[environment]
timezone = "America/New_York"   # 必填：主人的时区

[[input]]                       # 按顺序送入的消息，每条等这次 Run 结束再送下一条
session = "main"                # 可选，默认 main
text = "..."

[budget]
max_rounds = 24                 # 每个 Run 的上限，写进被测的配置
timeout_secs = 900              # 整道题的时限，到时取消并记为失败

[[criteria]]
id = "..."
grader = "state"                # state | rubric | command
check = "..."                   # 通过条件，英文
snapshot = "schedules.json"     # state：快照中的文件
schema = "hidden/schemas/x.json"   # state：路径相对题目目录

[[criteria]]
id = "..."
grader = "rubric"
check = "..."
sessions = ["main"]             # 评判看到哪些 Session 的对话；省略时为全部
evidence = ["mock/mail/state.json#/outbox"]   # 可选：一并交给评判的快照文件，`#` 后是 JSON Pointer，只取其中一部分
reference = """..."""           # 必填：参考答案，写明通过与不通过的界线

[[criteria]]
id = "..."
grader = "command"
check = "..."
run = "hidden/..."              # 参数是试验目录，以退出码给出结果，原因写到 stdout
```

- **`state`**：用 JSON Schema 校验快照中的一个文件。数量、常量、正则、包含这类结构化的事实都用它。
- **`rubric`**：运行器直接调用评判模型，一个评分项一次调用；提示词由运行器固定，请求与回复写进试验目录。评判先写理由，最后一行给出 `PASS`、`FAIL` 或 `UNKNOWN`。`UNKNOWN` 记为失败，因为它取决于被测的输出；回复不合格式或评判不可用是测量仪器的问题，不计入分母。评判不经被测的 Enco，测量工具才不会随被测对象改变。
- **`command`**：前两种表达不了时使用，例如编程题的契约测试。

输入消息按主人实际会说的语言写；`mechanism`、`check` 与 `reference` 是给运行器和评判模型的文字，用英文。

## mock 与占位符

每个 `hidden/mocks/<名字>.toml` 是一个 mock 服务，有自己的主机名，由静态路由与 JSON 集合组成：

```toml
host = "techdaily.io"            # 真实形态的主机名，经运行器的代理访问，不必真实存在

[[route]]
method = "GET"
path = "/feed/atom"
content_type = "application/atom+xml; charset=utf-8"
body = "hidden/mocks/techdaily/feed.atom"   # 默认状态码 200

[[collection]]
name = "inbox"
data = "hidden/mocks/mail/inbox.json"  # 初始数据；省略时为空
```

- 集合按 REST 的习惯访问：`GET /<名字>` 列出，查询参数按字段过滤（值按字符串比较），`fields=a,b` 只返回这些字段；`GET`、`PATCH`、`DELETE /<名字>/<id>` 读取、合并、删除；`POST /<名字>` 追加，正文可以是 JSON 或表单，没有 `id` 时依次分配。
- 收到的每个请求记入 `requests.jsonl`，结束时全部集合写入 `state.json`。
- 占位符：`{{mock:<名字>}}` 是 mock 的地址，即 `http://<host>`；`{{time:<偏移>}}` 与 `{{date:<偏移>}}` 是相对于试验开始时刻（取整到分钟）的时间与日期，按题目的时区写成 RFC 3339 与 `YYYY-MM-DD`。偏移可以组合 `d`、`h`、`m`、`s`，例如 `-3h05m14s`，也可以用 `b` 数工作日（周一到周五），例如 `+2b`；`0` 是开始时刻本身。日期可以加格式：`|zh` 写成"10 月 13 日（周二）"，`|en` 写成 "October 13, 2026"。`home/` 中的文本文件、路由的 `body` 文件与集合的初始数据在试验开始时替换；`reference` 在评分时按同一个开始时刻替换，评判因此看到与 Agent 相同的日期。数据里写具体日期而不写"本周四"，题目才不随试验落在星期几而变。
- 夹具命令（例如 `home/bin/mail`）用 POSIX sh 与 curl 编写，不引入其他运行时。运行器保留文件的可执行位，并把被测 home 的 `bin/` 放在 PATH 最前面，所以夹具命令优先于系统里的同名命令（例如 macOS 的 `/usr/bin/mail`）。

## 题目自检

`hidden/cases/<名字>/` 是评分的测试案例：`snapshot/` 中放评分要读的快照文件（可以只有一部分），`expect.toml` 写期望的结局：

```toml
start = 2026-10-11T09:00:00-04:00   # 案例假定的试验开始时刻；reference 用了时间占位符时必填
success = false        # 整道题是否成功；写了它就运行全部评分项，省略时只运行下面列出的
[criteria]
recall = "fail"        # 列出的评分项必须得到这个结局
```

- **oracle**：一份合格的结局，整道题必须成功。合格的写法不止一种时各写一份。
- **null**：什么都没做的结局，整道题必须失败。
- **近似错误**：只错一处，对应的评分项必须失败。

`runner check` 校验题目的格式与引用的文件，运行全部案例（rubric 的案例各评判两次，两次都必须符合期望），再把被测模型换成只回一句固定话的 mock，端到端跑一遍，整道题必须失败。新题进入题集前先试跑 k 次并阅读对话：每次都以同样的方式失败，多半是题目本身有问题；只有个别古怪的成功，多半有捷径。

## 题目质量清单

第 1–7 条保证有效性，第 8–9 条保证真实性，第 10 条决定题目的用途。

1. 写明这道题守护的能力，以及它要抓住哪种改坏。
2. 评分检查的一切都在题目中说明，写在输入里或 home 中主人的常驻指令里；两个认真的人读题会得出同一个判断。
3. 查结果，不查做法；只有过程本身就是验证对象时才查过程，例如经正式路径部署、只读手册。
4. 退化的做法必须失败：什么都不做、一律说不知道、什么都写上，都通不过整道题。null 与近似错误的案例检验这一条。
5. 能用 `state` 判断的不用 `rubric`；`rubric` 必须带参考答案。
6. 环境冻结：外部服务全部 mock，与时间相关的数据相对于试验开始生成，答案不能靠模型的先验知识猜出。
7. Agent 够不着答案：`task.toml` 与 `hidden/` 不进入 home，运行器检查工具调用没有触碰运行器所在的仓库与本次运行目录。
8. 题目来自主人真实的需求或真实发生过的失败；干扰与噪声是真实生活里本来就有的东西，例如订阅邮件、相似的人名与日期、关键词相同的无关新闻，而不是为了出陷阱生造的情节。
9. 不留测试的痕迹：Agent 看到的一切（输入、home 中的文件、mock 的数据、Session 名）都像真实的生活。不用 `.example` 这类保留域名、本机地址与端口、`owner@` 这类占位式的名字、按序号命名的 Session，也不用广为人知的示例名（例如 Northwind）；需要的机构与人名虚构但自然，不冒用真实的机构。Agent 一旦认出这是测试，测到的就不再是它平时的行为。
10. 守护题应当接近满分，目标题要留有余地；目标题饱和后转为守护题。

## 题目

| id | 类别 | 验证的机制 | 被测系统需要 |
|---|---|---|---|
| [`memory-update`](tasks/memory-update/task.toml) | 助理 | 记忆：跨 Session 准确召回，更正替换旧事实，没有依据时不猜 | 无，P0 的记忆就是基线 |
| [`daily-briefing`](tasks/daily-briefing/task.toml) | 助理 | 主人真实的每日早报：周期定时，经命令行工具收邮件、读新闻，遵守常驻偏好，汇报，发信，不执行邮件里的注入指令 | 无，M18 的周期定时就是基线 |

后续的 `adapter-drift`、`build-from-spec` 与 `many-tools` 题目设计见 [M25](../docs/plan/M25.md)，夹具与评分齐备后再加入可运行题集。

## 隔离

- 被测的 home 建在中性的位置：HOME 是一个以人名命名的目录，`ENCO_HOME` 是其下的 `.enco`，路径中不出现 evals、trial 或题目名；生成的 `config.toml` 与 `enco init` 的模板同形，不带运行器的注释。试验结束后，home 移入试验目录。Environment、`pwd` 与工具结果里出现的路径因此都像一台普通的电脑。
- 环境变量按白名单放行，宿主机上真实的凭据（例如邮件命令行工具的配置）对被测的守护进程不可见。题目因此可以使用看起来真实的地址与名字，误发的消息也到不了真实的收件人。
- 被测模型之外的外部服务一律经 mock：代理环境变量（大小写两种写法）指向运行器的代理，`NO_PROXY` 只列被测模型的地址。代理把发往 mock 主机名的请求交给对应的 mock，其余一律拒绝，写入 `network.jsonl`。在 Agent 看来它是一个普通的本地代理：拒绝时返回普通代理的 403 页面，不带运行器的字样。所以 mock 可以用真实形态的主机名，Agent 看到的是 `http://techdaily.io/feed/atom` 而不是本机的端口，隔离与真实感出自同一个机制。
- 运行器检查工具调用，确认 Agent 没有读取题目与运行结果。留出集的要求见[架构文档 §4.8](../docs/architecture/04-plugins.md)。
