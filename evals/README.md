# Enco 评测

本目录衡量 Agent 经正式路径（构建、部署、回退）改进自身能力的效果。从 P2 起，composer、工具与记忆策略的迭代以这里的任务成功率为目标，以 Attempt 记录的用量（含缓存命中）为成本约束（[路线图](../docs/Roadmap.md) P2，[验证的设计](../docs/design/verification.md)）。

题目各验证一个机制，用来确认机制走得通并建立基线，不是排行榜。题目会随 P2 的里程碑与使用增加，Agent 也可以补充自己的开发题。

## 运行

在仓库根目录构建被测程序与运行器：

```sh
cargo xtask build-factory
cargo build -p enco
cargo build --manifest-path evals/runner/Cargo.toml
```

命令都在仓库根目录运行；配置与变体中的路径相对于各自所在的文件：

```text
eval-runner run     --config <file> [--variant <file>] [--tasks <id,…>] [-k <n>] [--jobs <n>]
eval-runner compare --config <file> --base <file> --candidate <file> [--target <id,…>] [--tasks <id,…>] [-k <n>] [--jobs <n>]
eval-runner regrade <run-dir> [--judge]
eval-runner check   --config <file> [<id>…]
```

- `--tasks` 省略时为全部题目；`-k` 默认 5；`--jobs` 默认 1。
- `run` 与 `compare` 的结果写入 `target/evals/<run>/`，`<run>` 是开始时刻（UTC）加一段随机后缀。
- `regrade` 用已有的快照重新评分：不带 `--judge` 时，rubric 沿用已保存的评判回复；带 `--judge` 时重新评判。题目的内容与当时不同就拒绝，并指出是哪一道。
- `check` 运行题目自检（见下文）。
- 退出码：`run`、`compare`、`regrade` 完成即为 0，结论在 `result.json` 中；`check` 全部符合期望为 0，否则为 1；用法错误、配置错误或运行器自身出错为 2。

运行器配置（`--config`）：

```toml
model = "model.toml"                   # 被测的模型：只含 [endpoint.*]、[profile.*] 与 [memory] 的一段 Enco 配置

[judge]                                # 评判：OpenAI 兼容的 Chat Completions
base_url = "https://api.openai.com/v1"
model = "gpt-5.2"
api_key_env = "OPENAI_API_KEY"
temperature = 0                        # 可选，默认 0

[trial]                                # 可选
root = "/tmp"                          # 被测 home 的上级目录，默认系统临时目录；不能在仓库内
user = "lin"                           # HOME 目录的名字，默认 lin
path = "/usr/local/bin:/usr/bin:/bin"  # 被测守护进程 PATH 中 bin/ 之后的部分
```

- 运行器在被测模型的配置中加入题目的时区与预算，写成被测 home 的 `config.toml`。其中的 `api_key_env` 进入环境变量白名单，`base_url` 的主机进入 `NO_PROXY`。
- 评判模型与被测模型应属于不同的家族，避免评判偏袒自己一家的回答。评判的 key 只由运行器读取，不进入被测的环境。
- 配置不合法（包括缺少 key 变量）时退出码为 2，不开始任何试验。

变体（`--variant`、`--base`、`--candidate`）：

```toml
enco = "target/release/enco"     # 可选；默认 PATH 中的 enco

[[deploy]]                       # 可选：在每个被测 home 中部署的代际
plugin = "openai-compatible"
artifact = "plugin.wasm"
```

省略 `--variant` 时，变体是 PATH 中的 `enco`，运行的是出厂代际。变体的身份是二进制与各制品的 blake3 哈希，写进 `result.json`。部署失败说明变体本身有问题，整次运行停止，退出码为 2。

## 评测对进化的意义

Agent 改进自己的每一步，都要回答同一个问题：这次改动是不是更好。没有测量，改动只能凭感觉，改好与改坏无从分辨；有了评测，才有数据与指标，进化才有方向。评测因此是 Agent 自我进化的目标函数（[验证的设计](../docs/design/verification.md)）：Agent 读取 `result.json`，按"目标题上更好、其余没有倒退"决定采纳还是拒绝一次改动，失败的原因又是它下一个假设的来源。

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
- **产出是比较，而不是分数**：被测模型与评判模型固定，改动前后的两个变体在同一时间窗口内交错运行同一批题，结论回答进化时的问题：目标题上是否更好，其余每道题是否没有倒退。每道题运行 k 次，报告通过次数，以及 k 次全部成功的比例；判定规则见下文“结果”。

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
    transcript/<session>.md  对话：按顺序的消息，每条以 "## owner"、"## reminder" 或 "## assistant" 开头
    schedules.json  memory.json  plugins.json  status.json
    mock/<名字>/state.json   mock 的最终状态
    mock/<名字>/requests.jsonl
    network.jsonl            被代理拒绝的访问
  judge/<评分项>.json        评判的请求与回复
  grades.json
```

- `tool-calls.json` 按 Session 分组、按 Log 顺序列出每次调用：`session`、`round`、`call`、`name`、`arguments`（原始参数字符串）、`outcome`（`ok`、`failed` 或 `unknown`）、`code`（失败码）、`content`（模型看到的文本）。
- 对话文本不含工具调用与结果，需要它们的评分项读 `tool-calls.json`。
- `requests.jsonl` 每行是 `{ "at", "method", "path", "query", "body" }`，正文是 JSON 或表单时解析为对象；`network.jsonl` 每行是 `{ "at", "method", "target" }`。

## 结果

`result.json` 是 Agent 读取的接口，字段只增不改，`format` 是版本号：

```json
{
  "format": 1,
  "runner": "<crate 版本>",
  "started_at": "<RFC 3339>", "finished_at": "<RFC 3339>",
  "k": 5,
  "tasks": { "<题>": { "hash": "<blake3>" } },
  "model": { "hash": "<blake3>", "endpoints": { "<名字>": { "plugin": "…", "base_url": "…", "model": "…" } } },
  "judge": { "base_url": "…", "model": "…", "api_key_env": "…", "temperature": 0, "prompt": "<blake3>" },
  "variants": {
    "base": { "enco": { "path": "…", "hash": "<blake3>", "version": "…" },
              "deploy": [ { "plugin": "…", "artifact": "…", "hash": "<blake3>" } ] }
  },
  "trials": [ {
    "task": "…", "variant": "base", "index": 0, "dir": "trials/…/0",
    "start": "<RFC 3339>",
    "result": "success | failure | instrument_error",
    "reason": "…",
    "criteria": { "<评分项>": { "result": "pass | fail | error", "reason": "…" } },
    "rounds": 12, "wall_secs": 431.5,
    "usage": { "input_tokens": 0, "output_tokens": 0, "cached_input_tokens": 0 },
    "generations": { "<插件>": 2 },
    "network_violations": 0
  } ],
  "summary": { "base": { "<题>": { "passes": 4, "valid": 5, "instrument_errors": 0, "all_passed": false } } },
  "comparison": {
    "targets": [ "…" ],
    "verdict": "accept | reject",
    "reasons": [ "…" ],
    "p_better_targets": 0.024,
    "tasks": { "<题>": { "role": "target | guard", "base": [4, 5], "candidate": [5, 5],
                          "p_better": 0.5, "p_worse": 1.0,
                          "usage": { "base": { "…": 0 }, "candidate": { "…": 0 } } } }
  }
}
```

- `run` 的结果只有一个变体，名为 `base`，没有 `comparison`。
- 每个评分项的原因都在 `trials[].criteria` 中：失败的原因是 Agent 提出下一个假设的来源。
- `generations` 是试验结束时各插件的活跃代际；`model.endpoints` 以配置中的路径为键，例如 `endpoint.chat`。
- `usage` 汇总各 Attempt 报告的用量，有一次未报告缓存命中时为 `null`；`wall_secs` 不含评分。
- 执行中确定的失败（超时、越界等）记在试验目录的 `trial.json`，重评分保留它们。整次运行中止时不写 `result.json`。

**试验的结局**：被测对象启动之后的一切失败都算失败，包括守护进程起不来、`provider.*` 失败与超时。只有测量仪器自身的问题（home 建不起来、mock 无法监听、评判不可用或回复不合格式）记为 `instrument_error`，不计入分母。

**判定**（`α = 0.05`）：

- 每道题比较两个变体在有效试验中的通过次数，用超几何分布上的精确检验；多道目标题按题分层，只有一道时就是 Fisher 精确检验。
- 指定了目标题时，目标题上显著更好，并且每道非目标题都没有显著倒退，结论为 `accept`；不指定目标题时，每道题都没有显著倒退即为 `accept`，里程碑的不退化门禁用的就是这种形式。
- 任何一道题的任何一个变体没有有效试验时，比较没有依据，结论为 `reject`。
- 用量逐题报告两个变体的中位数，不进入结论。

灵敏度：5/5 → 2/5 的单侧 p 为 0.083，5/5 → 1/5 为 0.024，10/10 → 6/10 为 0.043。`k = 5` 只能发现接近彻底的倒退；要看得更细就加大 k，规则不变。

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
- 运行器检查工具调用，确认 Agent 没有读取题目与运行结果。留出集的要求见[验证的设计](../docs/design/verification.md)。
