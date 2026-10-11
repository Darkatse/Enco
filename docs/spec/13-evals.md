# 13 评测运行器（evals/runner）

运行器是 Enco 的黑盒客户端：它只启动 `enco`、调用它的命令行、读取管理命令输出的 JSON。它是独立的 Cargo 工作区（02 §2），引用不了宿主的 crate，依赖也与主工作区分开。它产出的是两个变体的比较，供 Agent 决定接受还是拒绝一次改动（架构文档 §4.8）。

题目的格式、mock 的写法、题目自检的案例与题目质量清单写在 `evals/README.md`，那是题目作者的权威说明；本章是运行器的契约，只在需要时引用它。

## 1. 命令

```text
eval-runner run     --config <file> [--variant <file>] [--tasks <id,…>] [-k <n>] [--jobs <n>]
eval-runner compare --config <file> --base <file> --candidate <file> [--target <id,…>] [--tasks <id,…>] [-k <n>] [--jobs <n>]
eval-runner regrade <run-dir> [--judge]
eval-runner check   --config <file> [<id>…]
```

- `--tasks` 省略时为 `evals/tasks/` 下的全部题目；`-k` 默认 `DEFAULT_K`；`--jobs` 默认 1。
- 命令在仓库根目录运行；配置与变体中的路径相对于各自所在的文件。
- `run` 与 `compare` 的结果写入 `target/evals/<run>/`，`<run>` 是开始时刻（UTC，`YYYYMMDD-HHMMSS`）加 4 位随机后缀。
- `regrade` 用已有的快照重新评分，重写 `grades.json` 与 `result.json`。不带 `--judge` 时，rubric 沿用 `judge/` 中已有的回复；带 `--judge` 时按 `result.json` 中的 `judge` 重新评判（key 的值从当前环境读取），重写 `judge/` 与提示词哈希。
- 退出码：`run`、`compare`、`regrade` 完成即为 0，结论在 `result.json` 中；`check` 全部符合期望为 0，否则为 1；用法错误、配置错误或运行器自身出错为 2。

## 2. 配置

`--config` 是一个 TOML 文件：

```toml
model = "model.toml"                   # 被测的模型：一段 Enco 配置，相对本文件

[judge]                                # 评判：OpenAI 兼容的 Chat Completions
base_url = "https://api.openai.com/v1"
model = "gpt-5.2"
api_key_env = "OPENAI_API_KEY"
temperature = 0                        # 可选，默认 0

[trial]                                # 可选
root = "/tmp"                          # 被测 home 的上级目录，默认系统临时目录
user = "lin"                           # HOME 目录的名字，默认 lin
path = "/usr/local/bin:/usr/bin:/bin"  # 被测守护进程 PATH 中 bin/ 之后的部分，默认即此值
```

- `model` 指向的文件只含 `[endpoint.*]`、`[profile.*]`、`[embedding]` 与可选的 `[decision]`（08 §3）；运行器在解析后的表中加入题目的时区与预算，再写入被测 home 的 `config.toml`。其中出现的 `api_key_env` 进入环境变量白名单，`base_url` 的主机进入 `NO_PROXY`。
- 评判模型与被测模型应属于不同的家族，避免评判偏袒自己一家的回答。评判的 key 只由运行器读取，不进入被测的环境。
- 配置不合法（包括缺少 key 变量）时退出码为 2，不开始任何试验。

## 3. 变体

`--variant`、`--base`、`--candidate` 都是变体文件：

```toml
enco = "target/release/enco"     # 可选，相对本文件；默认 PATH 中的 enco

[[deploy]]                       # 可选：在每个被测 home 中部署的代际
plugin = "openai-compatible"
artifact = "plugin.wasm"         # 相对本文件
```

- 省略 `--variant` 时，变体是 PATH 中的 `enco`，不部署任何东西，也就是出厂代际。
- 变体的身份是二进制与各制品的 blake3 哈希，写进 `result.json`。M21 起 `[[deploy]]` 可以带插件的设置。
- 部署失败说明变体本身有问题，整次运行停止并报告，退出码为 2。

## 4. 一次试验

一次试验是（题目, 变体, 序号）。`compare` 按题目交错安排两个变体的试验：同一道题的第 i 次，基准与候选相邻，所以两者落在同一个时间窗口内。

1. **开始时刻**：当前时刻按题目的时区取整到分钟，记为 `start`。占位符都相对于它（§8）。
2. **home**：在 `<trial.root>/<随机名>/<trial.user>/` 建 HOME，`ENCO_HOME` 为 `$HOME/.enco`。复制题目的 `home/`，保留文件权限；UTF-8 文本文件中的占位符被替换，其余文件原样复制。路径中不出现 evals、trial 或题目名。
3. **配置**：写 `$ENCO_HOME/config.toml`，与 `enco init` 的模板同形、不带注释：`timezone` 取题目的时区，`[run] max_rounds` 取题目的 `budget.max_rounds`，然后是被测模型的那段配置。
4. **mock 与代理**：启动题目的每个 mock 和一个代理，都监听 `127.0.0.1` 的随机端口（§7）。
5. **守护进程**：以下面的环境启动 `<enco> serve`，再反复执行 `enco status`，直到成功或超过 `DAEMON_READY_TIMEOUT`：
   - `HOME`、`ENCO_HOME`、`TMPDIR`（`$HOME/tmp`）、`LANG=C.UTF-8`；
   - `PATH` 为 `$ENCO_HOME/bin:` 加上 `trial.path`，题目的夹具命令因此优先于系统里的同名命令；
   - `http_proxy`、`HTTP_PROXY`、`https_proxy`、`HTTPS_PROXY` 都指向代理，`no_proxy` 与 `NO_PROXY` 只列被测模型的主机；
   - 被测模型所需的 key 变量。

   其余环境变量一律不传。
6. **部署**：对变体的每一项执行 `enco plugin deploy <plugin> <artifact>`。M24 起，再构建并部署题目 `home/plugins/` 下的夹具插件。
7. **输入**：按顺序对每条输入执行 `enco send --session <s> --wait <text>`（08 §7）。试验开始后超过 `budget.timeout_secs` 时，对正在进行的 Session 执行 `enco cancel`，等它的 `RunEnded`，这次试验记为失败，原因是超时。
8. **空闲**：全部输入之后，等 `enco status` 中没有正在执行的 Run。超过预算后，每次查询都取消仍在执行的 Run，它们须在 `SHUTDOWN_GRACE` 内结算。试验期间触发的定时会多出一个 Run，等它结束再取快照；快照由几条 CLI 查询依次导出，不是原子的。
9. **快照**：按 §5 导出。某条 enco 命令失败时，对应的文件不写，原因记入 `trial.json`，执行记为失败；其余导出继续。
10. **收尾**：守护进程自成一个进程组，工具启动的命令也在其中。向这个进程组发送 SIGTERM，超过 `SHUTDOWN_GRACE` 后 SIGKILL；停止 mock 与代理，导出它们的状态；把 HOME 移入 `trials/<题>/<变体>/<i>/home`，原路径记入 `trial.json`，删除本次试验的随机父目录。运行器被 SIGINT 或 SIGTERM 中断时，同样终止每次试验的进程组、删除临时 home，退出码为 2。
11. **评分**：按 §6 进行。

**只排除测量仪器自身的失败**：home 无法建立、mock 或代理无法监听，快照、mock 状态或 home 无法写出，记为仪器错误，这次试验不计入分母；评分阶段的仪器错误见 §6。被测对象启动之后的一切失败都算失败，包括守护进程起不来、Run 以 `provider.*` 失败和超时。

- 失败码由被测的插件自己报告，不能据此判断失败来自哪里，这与安全模式的进入是同一个理由（架构文档 §4.6）。如果按失败码排除试验，一个坏掉的候选 Provider 就能让自己的失败全部消失。
- 服务商的故障同时影响交错运行的两个变体，带来的是噪声而不是偏差，并且在失败原因中看得见。偏差会系统性地偏袒坏掉的候选，比噪声危险得多。

**等待有界**：运行器对被测对象的每次等待都有时限：管理命令 `CLI_TIMEOUT`，输入受预算约束，取消后 `SHUTDOWN_GRACE`。超时是这次试验的失败，坏掉的候选因此不会让整次比较停住。

**完整性**：工具调用的参数中出现运行器所在仓库的绝对路径，或者本次运行目录的绝对路径（按路径分量边界匹配），这次试验记为失败，原因是越界。读配置时拒绝位于仓库内的 `trial.root`，所以 Agent 正常工作不会触发它。

## 5. 快照

试验目录：

```text
trials/<题>/<变体>/<i>/
  trial.json                 开始时刻、home 的原路径、执行结局，以及导出命令失败的原因
  home/                      被测的 home（第 10 步移入）
  daemon.log                 enco serve 的 stderr
  snapshot/
    sessions.json            enco sessions
    log/<session>.json       enco log --session <session>
    tool-calls.json          由 Log 得出，见下
    transcript/<session>.md  由 Log 得出，见下
    schedules.json           enco schedules
    memory.json              enco memory
    plugins.json             enco plugin status
    status.json              enco status
    mock/<名字>/state.json   全部集合：{ "<集合>": [...] }
    mock/<名字>/requests.jsonl
    network.jsonl            被代理拒绝的访问
  judge/<评分项>.json        评判的请求与回复
  command/<评分项>.stdout    command 评分的输出（另有 .stderr）
  grades.json
```

`trial.json` 的 `execution.result` 是 `completed`、`failure` 或 `instrument_error`，后两种带 `execution.reason`；`export_errors` 按导出名称保存 CLI 错误。它们是重评分保留的执行事实。

**tool-calls.json**：一个数组，按 `sessions.json` 中的 Session 顺序分组，每组按 Log 中工具提出的顺序列出调用：

```json
{ "session": "main", "round": "<RoundId>", "call": "<CallId>", "name": "shell_exec",
  "arguments": "<原始参数字符串>", "outcome": "ok", "code": null, "content": "<模型看到的文本>" }
```

调用来自目的为 Reply 的 `AttemptSettled`（Completed）中的工具调用，按 `ToolCall.id` 与 `ToolCallSettled.call` 配对。`outcome` 取 `ok`、`failed`、`unknown`，`code` 是失败码。

**transcript/<session>.md**：按 Log 顺序列出对话，每条以一行标题开头，然后是正文，条目之间空一行：

- `## owner`：`EventConsumed` 中的 `UserMessage`，正文是它的 `text`；
- `## reminder`：`EventConsumed` 中的 `Reminder`，正文是它的 `text`；
- `## assistant`：目的为 Reply 的 `AttemptSettled`（Completed），正文是消息中的文本；只有工具调用、没有文本的不列。

工具调用与结果不进入对话文本；需要它们的评分项读 `tool-calls.json`。

**requests.jsonl** 每行是一个请求：`{ "at", "method", "path", "query", "body" }`。正文是 JSON 或表单时解析为对象，否则为字符串。**network.jsonl** 每行是一次被拒绝的访问：`{ "at", "method", "target" }`。

## 6. 评分

每个评分项只读快照目录与开始时刻，所以评分可以在守护进程停止之后进行，`regrade` 因此成立。结局是 `pass`、`fail` 或 `error`，带一段原因（至多 `REASON_BYTES`）。`error` 只表示测量仪器自身出了问题：评判不可用或回复不合格式、评分程序异常、Schema 无效。

任何一项为 `fail` 时，这次试验失败；没有 `fail` 而有 `error` 时，结局无法确认，记为仪器错误，不计入分母；全部为 `pass` 时成功。结果写进 `grades.json`：`{ "<评分项>": { "result", "reason" } }`。

所有评分项需要的快照或证据缺失时为 `fail`，因为被测对象没有交出它；导出命令失败的原因在 `trial.json` 中。

**state**：读取 `snapshot` 指向的文件，以 JSON Schema 2020-12 校验 `schema` 指向的 Schema。通过为 `pass`；不通过为 `fail`，原因是校验器给出的前几条错误。Schema 无效为 `error`。

**rubric**：向评判发出一次请求。

- 请求：`POST {base_url}/chat/completions`，`temperature` 取评判配置，消息只有两条。系统消息是下面的固定提示词；用户消息依次列出 `check`、按开始时刻替换占位符后的 `reference`、`sessions` 中每个 Session 的对话文本，以及 `evidence` 中每个文件（带 `#` 的取 JSON Pointer 指向的部分，格式化为 JSON）：

  ```text
  ## Criterion
  {check}

  ## Reference
  {reference}

  ## Transcript: {session}
  {transcript}

  ## Evidence: {path}
  {json}
  ```

- 系统消息（源码中的常量，它的 blake3 哈希写进 `result.json`）：

  ```text
  You are grading one criterion of an evaluation of a personal assistant. You see the criterion, a reference written by the task author, and the evidence: conversation transcripts and files from the assistant's environment after the run.

  Judge only this criterion, against the reference. The reference states what passes and what fails; follow it even where you would judge differently, and add no requirements of your own. Paraphrase, translation, formatting and length do not matter unless the reference says so.

  First explain your reasoning briefly, citing the evidence. Then end with a line containing exactly one word: PASS if the evidence meets the criterion, FAIL if it does not, or UNKNOWN if the evidence is insufficient or the reference does not decide the case.
  ```

- 回复：最后一个非空行是 `PASS`、`FAIL` 或 `UNKNOWN` 时即为结局，其余内容作为原因。`UNKNOWN` 记为 `fail`，原因中注明：它取决于被测的输出，证据不足通常说明 Agent 没有交出应有的东西；参考答案本身判定不了的情况，由题目自检的案例发现（§11）。最后一行是其他内容，或者 HTTP 失败重试 `JUDGE_RETRIES` 次后仍失败，为 `error`。
- 请求与回复写入 `judge/<评分项>.json`。每次请求的时限为 `GRADER_TIMEOUT`。

**command**：在题目目录下执行 `run`，参数是试验目录，时限 `GRADER_TIMEOUT`。退出码 0 为 `pass`，1 为 `fail`，其他或超时为 `error`；原因取 stdout 的末尾。

## 7. mock 与代理

**mock**：每个 `hidden/mocks/<名字>.toml` 在每次试验中启动一份，状态互不相通。

- `host` 必填，是 mock 的主机名；`{{mock:<名字>}}` 替换为 `http://<host>`。
- `[[route]]`：`method` 与 `path` 精确匹配，返回 `status`（默认 200）、`content_type` 与 `body` 文件的内容（已替换占位符）。
- `[[collection]]`：`name` 与可选的 `data`（JSON 数组，已替换占位符）。
  - `GET /<集合>` 返回数组：除 `fields` 外的查询参数按字段相等过滤，值按字符串比较；`fields=a,b` 只返回这些字段。
  - `GET /<集合>/<id>` 返回一项，不存在为 404。
  - `POST /<集合>` 追加一项，返回 201 与该项。正文是 JSON 对象或表单，表单的值都是字符串；没有 `id` 时取现有数字 `id` 的最大值加一。
  - `PATCH /<集合>/<id>` 合并字段，返回 200 与该项。
  - `DELETE /<集合>/<id>` 删除，返回 204。
- 其他请求返回 404。每个请求都追加到 `requests.jsonl`，结束时全部集合写入 `state.json`。

**代理**：每次试验一个，是一个 HTTP/1.1 正向代理。

- 绝对形式的请求（`GET http://<host>/<path>`），主机是本次试验某个 mock 的 `host` 时，转给该 mock。
- 其余请求，包括所有 `CONNECT`，返回 `403 Forbidden`，正文是一段普通的 HTML（`Request blocked by proxy policy.`），并记入 `network.jsonl`。
- 被测模型的主机在 `NO_PROXY` 中，不经过代理；虚构的 mock 主机名因此不需要能解析。
- 代理挡不住直接打开的 socket，它让"外部服务一律经 mock"成为可以检查的事实。

## 8. 占位符

```text
{{mock:<名字>}}             http://<host>
{{time:<偏移>}}             RFC 3339，带题目时区的偏移，例如 2026-10-11T05:54:46-04:00
{{date:<偏移>}}             YYYY-MM-DD
{{date:<偏移>|zh}}          10 月 13 日（周二）
{{date:<偏移>|en}}          October 13, 2026
```

- 偏移是 `0`，或者带符号的若干段 `<n>d`、`<n>h`、`<n>m`、`<n>s` 的组合（例如 `-3h05m14s`），或者带符号的 `<n>b`。
- `d` 按当地日历移动，`b` 按工作日（周一到周五）移动；保留当地时刻，重复时刻取较早的一个，夏令时缺口取缺口之后第一个有效时刻。
- `h/m/s` 按绝对时长移动，组合偏移按书写顺序计算；`0` 保留原时刻。输出使用题目的时区。
- 替换发生在三处：试验开始时替换 `home/` 中的文本文件、路由的 `body` 文件与集合的初始数据；评分时替换 `reference`。三处都用同一个开始时刻。
- 未知的 mock 名或无法解析的偏移在加载题目时拒绝：`check` 报题目错误并继续其他题；`run` 与 `compare` 在开始试验前退出，退出码为 2。

## 9. 比较与判定

对每道题 t，记基准在有效试验 n_a 次中通过 a 次，候选在 n_b 次中通过 b 次。在"两个变体的成功率相同"的零假设下，固定总通过数 s = a + b，候选的通过次数服从超几何分布 B ~ Hypergeom(N = n_a + n_b, K = s, n = n_b)：

- `p_better(t) = P(B ≥ b)`，`p_worse(t) = P(B ≤ b)`；
- 多道目标题按题分层：统计量 T = Σ b_t，零假设分布是各题超几何分布的卷积，`p_better(targets) = P(T ≥ Σ b_t)`。只有一道目标题时，它就是 Fisher 精确检验。

前提：任一道题的任一变体没有有效试验时，比较没有依据，结论为 `reject`，原因是"没有有效样本，无法比较"。否则零样本的超几何分布退化为 B ≡ 0，`p_worse` 恒为 1，没有证据也会被当作没有倒退。

结论（`ALPHA = 0.05`）：

- 指定了目标题时，`p_better(targets) < ALPHA`，并且每道非目标题都有 `p_worse ≥ ALPHA`，结论为 `accept`；否则为 `reject`，并列出不满足的条件与题目。
- 不指定目标题时，每道题都有 `p_worse ≥ ALPHA` 即为 `accept`。里程碑的不退化门禁用的就是这种形式。

用量（输入、输出与命中缓存的 token，Round 数，耗时）逐题报告两个变体的中位数，不进入结论。

灵敏度：

| 基准 → 候选 | 单侧 p |
|---|---|
| 5/5 → 2/5 | 0.083 |
| 5/5 → 1/5 | 0.024 |
| 4/5 → 0/5 | 0.024 |
| 10/10 → 7/10 | 0.105 |
| 10/10 → 6/10 | 0.043 |

`k = 5` 只能发现接近彻底的倒退；要看得更细就加大 k，规则不变。逐题检验有多重比较：5 道题都没有真实变化时，至少一道被误判为倒退的概率不超过约 23%，Fisher 检验是离散的，实际更低。误报的实测由 A/A 比较给出，写在 §11。

**可比性**：`compare` 在同一次运行中交错两个变体，题目、被测模型与评判天然相同，没有拼接旧成绩的入口。`regrade` 先核对题目哈希（题目目录全部文件的路径、内容与可执行位），不同就拒绝，并指出是哪一道。

## 10. result.json

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
- `generations` 是每次试验结束时各插件的活跃代际（取自 `plugins.json`）。
- `model.endpoints` 以配置中的路径为键，例如 `endpoint.chat`、`embedding`。
- 每个评分项的原因都写进 `trials[].criteria`：失败的原因是 Agent 提出下一个假设的来源。
- 执行中确定的失败（超时、越界等）记在 `trial.json`，重评分保留它们。
- `usage` 汇总各 Attempt 报告的用量，有一次未报告缓存命中时为 `null`；`wall_secs` 不含评分。
- `result.json` 在全部试验结束后写出一次；整次运行中止时不写。

## 11. 题目自检（check）

1. **静态**：
   - `task.toml` 与 `hidden/mocks/*.toml` 能解析，引用的文件都存在，Schema 能编译，占位符能解析；
   - 每个 rubric 都有 `reference`；
   - 案例的 `expect.toml` 只引用存在的评分项；
   - `reference` 用了时间占位符时，案例写明了 `start`。
2. **案例**：对每个案例，以它的 `snapshot/` 为快照、以它的 `start` 为开始时刻，运行列出的评分项（写了 `success` 时运行全部），结局必须与期望相同。rubric 的案例评判两次，两次都必须相同。
3. **空跑**：把被测模型换成一个对每个请求都回答"好的。"的 mock：补全返回这句话，嵌入返回 `NULL_EMBEDDING_DIMENSIONS` 维的零向量；它经代理以一个虚构的主机名访问。用这个模型完整跑一次试验：执行必须正常结束，整道题由评分项判为失败。守护进程没能运行的空跑不算通过，运行器自身的配置问题因此在比较之前暴露。
4. 打印每一项的结果，有任何不符时退出码为 1。

A/A 比较的误报实测（M19）：两道题各 k = 5，0/2 道被判为倒退，保留 `DEFAULT_K = 5`。一轮未误报不是长期误报率的估计，灵敏度以 §9 为准。

## 12. 文件布局

```text
evals/runner/
  Cargo.toml     独立工作区：依赖（02 §4）与 lint 表的副本（02 §5）
evals/runner/src/
  main.rs        clap 命令与退出码
  run.rs         安排试验、汇总与重评分
  config.rs      运行器配置与变体
  task.rs        题目加载与校验、mock 定义、题目哈希
  render.rs      占位符
  home.rs        建 home、写 config.toml、环境变量
  enco.rs        启动与停止守护进程，调用 enco 的命令行
  enco/output.rs 运行器消费的 CLI 输出类型
  server.rs      mock 与代理共用的 HTTP 监听
  mock.rs        mock 服务
  proxy.rs       代理
  trial.rs       一次试验的流程（§4）
  snapshot.rs    快照与派生文件（§5）
  grade.rs       三种评分
  judge.rs       评判的请求与解析
  stats.rs       超几何分布、分层检验与结论（§9）
  result.rs      result.json 与 trial.json
  check.rs       题目自检（§11）
  limits.rs      常量（§13）
```

## 13. 常量

| 常量 | 值 |
|---|---|
| `DEFAULT_K` | 5 |
| `ALPHA` | 0.05 |
| `DAEMON_READY_TIMEOUT` | 30 s |
| `CLI_TIMEOUT` | 60 s |
| `SHUTDOWN_GRACE` | 10 s |
| `GRADER_TIMEOUT` | 300 s |
| `JUDGE_RETRIES` | 2 |
| `REASON_BYTES` | 2000 |
| `NULL_EMBEDDING_DIMENSIONS` | 8 |
| `STATUS_INTERVAL` | 100 ms |
