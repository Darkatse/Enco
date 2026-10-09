# 05 宿主：原生工具、上下文源与出厂 composer（enco-host）

enco-host 为内核端口提供原生适配器：`SqliteStore`（03 §3）、原生工具、`InstructionsContextSource`、`FactoryComposer`、`SystemClock`，以及记忆（06）。这里是目前所有"策略"与"排版"所在的地方。

## 1. 工作区

```text
$ENCO_HOME/workspace/        工具的默认工作目录；Agent 的"家"
```

工作区目前是普通目录，**不初始化 git**（git 集成属于 P2），也不纳入 `$ENCO_HOME` 的版本管理（08 §2）。守护进程启动时确保 `workspace/` 存在。主人的常驻指令是仓库根目录的 `$ENCO_HOME/AGENTS.md`（可选，Agent 可以编辑），它属于主人的意图，不在工作区中。记忆也不在工作区中（06）。

路径规则（所有文件工具一致）：相对路径相对于工作区解析；允许绝对路径（当前不设沙盒）；不做 `~` 展开。

## 2. 原生工具（`tools/`）

```rust
/// 返回全部原生工具。`workspace` 用于解析相对路径和作为 shell 的默认工作目录。
pub fn native_tools(workspace: PathBuf) -> Vec<Arc<dyn Tool>>;
/// 插件管理工具；与 CLI 调用同一组 Registry 方法（11 §8）。
pub fn plugin_tools(registry: Arc<Registry>, workspace: PathBuf) -> Vec<Arc<dyn Tool>>;
/// 救生集：安全模式下唯一可用的工具，也是 requires_lifeline 的 profile 要求常驻的工具。
pub const LIFELINE: [&str; 8] = ["fs_read", "fs_write", "fs_edit", "fs_list", "shell_exec",
                                 "plugin_status", "plugin_deploy", "plugin_rollback"];
```

所有原生工具的 `code()` 为 `CodeRef::Native { name: "host-tools", version: crate 版本 }`。

参数缺失、类型错误或值不合法时，返回 `Failed { code: "tool.invalid_arguments", retryable: false }`，message 指出是哪个参数。执行中的错误返回 `Failed { code: "tool.failed" }`，message 包含路径与底层错误。

文件工具（`fs_*`）不响应取消：本地文件 IO 很快，完成后照常结算。FIFO、设备文件这类会阻塞的特殊文件例外，取消与关闭都要等读写返回。

### 2.1 fs_read（ReadOnly）

| 参数 | 类型 | 说明 |
|---|---|---|
| `path` | string | 必填 |
| `offset` | integer ≥ 1 | 起始行号，默认 1 |
| `limit` | integer ≥ 1 | 最多读取的行数，默认 2000，上限 10000 |

结果：`Ok { value: "<内容>" }`，内容前加一行说明。按行读取，最多 `limit` 行；再加一行就会让结果（含说明行）超过 `ctx.result_budget` 时，停在这一行之前，但至少返回一行，单行超过预算时由内核截断兜底（04 §6.6）。读到文件末尾时说明行为 `[lines {a}-{b} of {total}]`，否则为 `[lines {a}-{b} of {total}; continue at offset {b+1}]`。工具描述写明结果可能在 `limit` 之前结束，说明行给出继续读取的 offset。文件不是合法 UTF-8 时返回 `tool.failed`。起始行超出文件末尾时返回空内容和同样的说明行。

### 2.2 fs_write（Idempotent）

参数：`path`、`content`。自动创建父目录；原子写入（同目录临时文件 + `rename`）。结果：`Ok { value: { "path": <实际写入的绝对路径>, "bytes": n } }`。

原子写入由 `fs_write` 与 `fs_edit` 共用，替换已有文件时保留它的身份：先解析到真实路径（跟随符号链接），临时文件写在真实路径所在目录，并带上原文件的权限，再 `rename` 到真实路径。因此符号链接保持不变，可执行权限不会丢失。新文件使用默认权限。属主与属组不处理（需要特权）。

### 2.3 fs_edit（SideEffect）

参数：`path`、`old_string`、`new_string`、`replace_all`（布尔，默认 false）。

- `old_string` 必须非空，且与 `new_string` 不同。
- `replace_all` 为假时，`old_string` 必须恰好出现一次：出现零次返回 `tool.failed`（`old_string not found in {path}`）；出现多次返回 `tool.failed`（`old_string occurs {n} times in {path}; include more context or set replace_all`）。
- 原子写入。结果：`Ok { value: { "path": …, "replacements": n } }`。

### 2.4 fs_list（ReadOnly）

参数：`path`。不递归。结果：`Ok { value: [ { "name", "type": "file" | "dir" | "symlink", "size" } ] }`，按名称排序。

### 2.5 shell_exec（SideEffect）

| 参数 | 类型 | 说明 |
|---|---|---|
| `command` | string | 由 `sh -c` 执行 |
| `cwd` | string | 可选，默认工作区 |
| `timeout_ms` | integer | 可选，默认 120000，上限 1800000 |

行为：

- `tokio::process::Command::new("sh").arg("-c").arg(command)`，stdin 为 null，stdout 与 stderr 分别捕获，各保留前 1 MiB（超出部分丢弃，并在末尾注明 `[truncated]`），按 lossy UTF-8 解码；设置 `kill_on_drop(true)`。
- 用 `select!` 同时等待：进程结束、两个管道读完、超时、`ctx.cancel`。
- 进程结束且两个管道都已读完：`Ok { value: { "exit_code": <整数或 null>, "stdout": …, "stderr": … } }`。非零退出码仍是 `Ok`：命令确实执行了，结果如实交给模型判断。
- 后台子进程会继承管道，使它们在 `sh` 退出后仍不关闭。`sh` 退出后最多再读取 `SHELL_EXIT_GRACE`；管道仍未关闭时照样返回 `Ok`，带上已收集的输出和 `note`，说明后台进程可能仍在运行，并建议把它的输出重定向到文件（`cmd > out.log 2>&1 &`）。工具描述中有同样的提示。
- 超时：kill 进程并**等待它退出**，然后返回 `Unknown { code: "timeout", message: "command timed out after {n} ms and was killed; it may have partially run" }`。
- 取消：kill 进程并等待它退出，然后返回 `Unknown { code: "cancelled", message: "command was cancelled and killed; it may have partially run" }`。
- 无法启动：`Failed { code: "tool.failed" }`。

已知限制：目前只终止 `sh` 本身，命令派生的后台孙进程可能存活；没有重定向输出的后台进程在工具返回后写管道时会收到 `SIGPIPE`。进程组的处理留到后续阶段（需要新增依赖），不要自行引入。

### 2.6 插件工具（`tools/plugins.rs`）

它们把 Agent 的调用交给注册表（11），与记忆工具包住 `Memories` 是同一个模式；读文件在这里做，`Registry::deploy` 只收字节。

| 名称 | 参数 | 结果 | Effect |
|---|---|---|---|
| `plugin_status` | 无 | `Ok { [PluginStatus] }`（11 §4） | ReadOnly |
| `plugin_deploy` | `name`；`path`：组件文件，相对路径相对于工作区 | `Ok { generation, users }`；文件读不到 → `tool.failed`；`Rejected` → `plugin.rejected` | SideEffect |
| `plugin_rollback` | `name` | `Ok(GenerationRecord)`，与 CLI 相同；`NoRollbackTarget`、`Conflict` → `plugin.rejected`，message 原样带出 | SideEffect |

`UnknownPlugin` 是参数问题，返回 `tool.invalid_arguments`。描述的要点见 11 §8。

工具描述与省略原因中出现的上限（默认行数、超时、宽限时间、预算比例、记忆文本长度等）都由 `limits.rs` 的常量生成，不另写数字。

## 3. InstructionsContextSource（`context.rs`）

实现 `ContextSource`，不使用 `ContextQuery` 中的 Event。每次调用都重新读取文件，所以对常驻指令的修改在下一个 Round 立即生效。

- `InstructionsContextSource::new(path)`，`path` 为 `$ENCO_HOME/AGENTS.md`。文件存在时 → `Candidate { id: "instructions:AGENTS.md", kind: Instruction, text, standing: true }`；不存在不是错误，只是没有候选。
- 文件不是合法 UTF-8 时返回 `ContextError`，message 指明文件。宁可让这一轮明确失败，也不要悄悄跳过主人的指令。

## 4. FactoryComposer（`composer.rs`）

出厂 composer，也就是安全模式使用的 composer。`FactoryComposer::new(workspace: PathBuf, instructions: PathBuf)`，`code()` 为 `CodeRef::Native { name: "factory-composer", version: crate 版本 }`。

它是纯函数：只读取 `ComposeInput` 和构造时传入的工作区、指令文件路径。

### 4.1 请求的形状

按 Agent 读到的顺序，一次回复请求是：

1. **System 消息**：Agent 是谁、怎样工作，以及它始终要知道的事：环境、主人的常驻指令、自己的置顶记忆、自己对更早对话的摘要。
2. **对话**：Transcript 的条目按 Log 顺序排列。来自 Inbox 的输入前面可以有一条说明，写着这条消息何时到达、之前的工作为何停止、Agent 因它想起了哪些记忆（§4.3）。
3. 请求在最新的条目处结束：主人的消息、通知或工具结果。Agent 最后读到的就是它要回应的内容。

**只追加。** 说明一经写入就留在原位：composer 沿用本 Session 上一份回复计划（`ComposeInput.previous_plan`，04 §2），只在末尾添加新条目和它们的说明。所以 Agent 以后看到的历史，就是它当时看到的样子；每次请求都以上一次请求的全部内容开头，前缀缓存一直延伸到上一次的末尾。实际命中多少记在 `AttemptSettled` 的 `usage.cached_input_tokens` 中。

上一份计划存在、它的 System 消息及其来源与本次相同、它引用的 Log 条目都还在 Transcript 中时沿用；否则从头渲染，开始新的系列。System 消息只在常驻指令、置顶记忆、摘要、时区或安全模式改变时变化，压缩会让旧条目离开 Transcript。

System 消息由以下各节依次拼接，没有内容的节整节省略：

```text
{prompts/system.md；安全模式时改用 prompts/safe_mode.md}

## Environment
- Workspace: {工作区绝对路径}
- Standing instructions: {AGENTS.md 的绝对路径}
- Session: {session.name}
- Time zone: {主人的时区，IANA 名称} (UTC{该时区在 now 时的偏移，±HH:MM})

## Standing instructions (AGENTS.md)
{常驻的 Instruction 候选}

## Your pinned memories
- {内容} (id: {候选 id 去掉 "memory:" 前缀})
…

## Your summary of the earlier conversation
{transcript.summary}
```

候选放在哪里由 `standing` 决定（03 §1.8）：常驻候选不看最新输入也会提供，放进 System 消息；其余候选因最新输入而召回，放进这条输入的说明。出厂 composer 认识的有三种：常驻的 Instruction 进常驻指令一节，常驻的 Memory 进置顶记忆一节，召回的 Memory 进说明。

说明是一条 User 消息，放在它所属的输入之前，各部分之间空一行，没有内容的部分省略，整条都没有内容时不放：

```text
[{YYYY-MM-DD HH:MM}, {英文星期}]

{上一次工作未正常完成的说明}

{输入来源的说明}

Memories you recall for the next message:
- {内容} (id: {候选 id 去掉 "memory:" 前缀})
…
```

说明是 Agent 对这条输入的处境感知，不是主人说的话。它和提醒、插件回退通知一样用 User 角色（03 §1.4），因为一些协议只在开头接受 system，一些聊天模板也会把所有 system 消息合并到开头。说明逐字记在计划中，不是规范消息；自动召回块不作为压缩原文，时间、工作结局与输入来源则由同一条渲染路径用于回复和压缩。

### 4.2 预算

所有估算都使用 `estimate_tokens`（03 §1.11）。回复的预算是 `profile.reply.budget`，压缩的预算是 `profile.compaction.budget`（12 §5）；本节与 §4.3、§4.4 中的 `context_tokens`、`max_output_tokens` 都指回复预算。

- **常驻指令**：总估算不超过 `context_tokens` 的 10% 时全部纳入，否则整份省略，记录 `Omission { source: id, reason: "instructions exceed 10% of the context window" }`。
- **记忆**：本 Round 要显示的记忆，即置顶记忆和说明中新出现的召回记忆，按候选的顺序（即来源给出的优先级，06 §5）逐条纳入，累计不超过 `context_tokens` 的 15%；超出的每一条都记录 `Omission { source: id, reason: "memory budget (15% of the context window) exceeded" }`。
- `plan.omitted` = `input.context.omitted`（来源报告的，原样放在最前）++ 上面两条规则产生的省略。

### 4.3 Reply 计划

```text
head = System 消息（§4.1）
if previous_plan 存在，它的第一项是与 head 相同的 System 消息（含来源），它引用的 Log 条目都在 transcript 中:
    items = previous_plan.items                  // 沿用本系列
    新条目 = transcript 中位置大于它最后一个 Log 引用的条目
else:
    items = [Message { System, head }]           // 开始新系列
    新条目 = transcript 的全部条目
for 新条目中的每一项:
    说明不为空时放入 Message { User, 说明 }
    放入 Log { pos }
tools = input.tools 全部，原样复制（不做渐进式披露，那是 P2 默认 composer 的职责）
max_output_tokens = profile.reply.budget.max_output_tokens
```

只有来自 Inbox 的输入（带 `event` 的条目，04 §2）有说明：

- **时间**：接收时间按主人的时区（`ComposeInput.timezone`）换算为当地时间，每个时刻用它自己当时的偏移，所以夏令时切换前后的输入各自显示正确的当地时间。这条输入是 Transcript 中的第一条输入，或者与上一条输入不在同一个钟点（当地日期、小时与 UTC 偏移不全相同）时，说明以时间开头。相隔一小时以上的两条输入必然落在不同的钟点，所以这条规则既标出隔了很久才来的消息，也在持续的对话中大约每小时标一次。时间只取决于已记录的接收时间和主人的时区，从头渲染也得到同样的结果。
- **工作结局**：按 Log 位置，将 `transcript.run_ends` 中的每个结局归到其后的第一条输入。两条输入之间最近的结局不是 `Completed` 时，说明写明原因。一次接纳多条输入时，只在第一条输入前写；同一输入的工具循环沿用已有说明。从头渲染时仍由相同事实确定，已压缩的结局随摘要保留。
- **输入来源**：输入不是主人发来的时，说明写明它的来源，判据是 Event 的类型。`Reminder` 写 `The next message is a reminder you scheduled for {YYYY-MM-DD HH:MM, 英文星期}.`，触发时刻按主人的时区、与时间标记同一格式，`skipped` 大于 0 时接着写 ` Missed earlier occurrences: {skipped}.`；`GenerationRolledBack` 写 `The next message is a notice from the plugin registry.`。
- **召回的记忆**：只出现在 Transcript 中最新一条输入的说明里，而且只在这条说明新渲染时出现。只与该 id 最近一次展示的内容比较：相同则不重复，更正后在后续新输入的说明中作为新的一行出现，改回更早的值也一样。Transcript 中没有输入（都已被压缩）时不显示。

记忆源只在本 Round 接纳了新输入时召回（06 §5）。所以工具循环中途从头渲染时（压缩或 System 消息改变），最新输入的说明没有召回的记忆，直到下一条输入。

每条内联消息的 `sources` 记录实际使用的候选 id 与原始文本哈希。去重从上一份计划的消息及来源倒序查找该 id 最近一次出现的哈希。System 消息同样记录纳入的常驻候选；只有时间、工作结局、输入来源的说明与压缩消息的候选来源为空。

请求中没有当前时间：Environment 给出时区，Agent 需要精确时间时自己查询（§4.6 的 `system.md`）。时间只到分钟、跨整点才标，是因为 Agent 读时间不需要更细。标记不带偏移：参照系就是 Environment 中的时区，时区或偏移改变时 System 消息随之改变，全部标记从头渲染。显示哪些接收时间、怎样显示是 composer 的策略，例如角色扮演的 composer 可以改用故事中的时间。写下后原样回放的时刻不随 Environment 重新渲染，所以保留偏移，并使用工具参数的 RFC 3339 格式，例如定时工具的结果（04 §11）。

工作结局的文案由 composer 持有：`Your previous work {原因} before it finished. The tool results above show what you did and which actions remain uncertain.` 原因依次为 `was interrupted`（`Interrupted`）、`was cancelled`（`Cancelled`）、`reached the step limit`（`BudgetExhausted`）、`failed ({code}: {message})`（`Failed`）。

### 4.4 何时压缩

```text
total = est(System 消息) + Σ est(历史条目) + est(serde_json::to_string(tools 的 spec)) + max_output_tokens
est(历史条目) = est(serde_json::to_string(item.message))，有说明时再加上 est(serde_json::to_string(说明消息))

if total <= context_tokens * 80%:
    返回 Plan
keep = context_tokens * 30%
b = 压缩边界（见下）
if b 存在 且 compactions_left > 0:
    返回 Compact { upto: b, plan: compaction_plan(b) }
elif total <= context_tokens:
    返回 Plan                                   // 超过阈值但仍然放得下
else:
    返回 Err(ContextOverflow { needed: total, window: context_tokens })
```

历史的预算按本次最终计划计算，包括沿用的旧说明；说明与它后面的 Log 引用一起计入该条目的大小。15% 只约束本轮新选择的记忆，不豁免旧说明对窗口的占用。

压缩边界按 `round_ends` 从早到晚选：首选最早一个使 `Σ est(pos > b 的历史条目) <= keep` 的位置；如果摘要到那里放不进压缩窗口（§4.5），就选仍然放得下的最晚位置，先压缩一部分。每份摘要都带着上一份，所以部分压缩也是进展，同一个 Round 还有压缩次数时会接着压缩。连第一个 Round 都放不进压缩窗口时，没有边界。

### 4.5 压缩计划

为了不依赖各家服务商对"历史中有工具调用但请求不带工具"的处理差异，压缩请求把待摘要的历史渲染成纯文本：

```text
items = [
  System: prompts/compaction.md,
  User:   "Previous summary:\n{transcript.summary}\n\n"（有旧摘要时）
          + "Conversation to summarize:\n" + 按顺序渲染 pos <= b 的条目：
              输入说明        → 时间、工作结局与输入来源，和回复共享 §4.3 的渲染，放在所属输入之前；不含自动召回记忆
              输入            → "{说话人}: {text}"，说话人与输入来源同一判据：`UserMessage` 为 Owner，`Reminder` 为 Reminder，`GenerationRolledBack` 为 Notice
              Assistant 文本  → "You: {text}"
              工具调用        → "You called {name} with {arguments，最多 2000 字节}"
              Tool 消息       → "Result of {name}: {content，最多 2000 字节}"
          + "\n\nWrite the summary now."
]
tools = []
max_output_tokens = min(2048, profile.compaction.budget.max_output_tokens)
```

压缩计划的估算总量（两条消息加输出上限）不超过 `profile.compaction.budget.context_tokens`：§4.4 选边界时就按这个窗口计算，所以压缩模型可以比回复模型小。窗口小只会让每次摘要的历史变少、压缩次数变多；只有一个 Round 的历史都放不进压缩窗口，而回复本身也放不下时，才返回 `ContextOverflow`。

### 4.6 提示词（`prompts/`，用 `include_str!` 嵌入）

`system.md`：

```text
You are Enco, a personal assistant working for one owner. You run on the owner's machine with full access to the file system and the shell, so act deliberately: read before you write, and say what you are about to do before any action that is hard to undo.

Reply in the language the owner uses. Be concise.

What you see
- A short note may come just before an incoming message. When the message is the first one shown or arrives in a new hour, the note starts with its arrival time, such as [2026-10-02 14:03, Friday]; an unmarked message arrived in the same hour as the one before it. The note may also explain why your previous work stopped, say that the message is a reminder you scheduled or a notice from the plugin registry, or list memories you recall for that message. These notes describe your own circumstances, not the owner's words.
- The conversation ends with the newest message or tool result; continue from there.

Memory
- Your memories are durable facts about the owner. Your pinned memories, listed below, are current; other memories appear in the note before the message they bear on and show what you recalled then. Memories override older things said in the conversation; when the owner tells you something newer, update the memory.
- When you learn something worth keeping (a preference, a person, a commitment, an important event), save it with memory_save as one self-contained statement. Pin only facts that matter in almost every conversation.
- To correct a memory, call memory_update with its id so that the old statement is replaced. To forget one, call memory_forget.
- You see only some of your memories. Use memory_search when something may have been saved before; it shows each memory as it is now.

Standing instructions from the owner live in the AGENTS.md file listed under Environment. Edit it when the owner asks you to change how you work.

Tools
- A tool result marked "outcome unknown" means the action may already have happened. Check the current state before trying again.
- For the exact current time, for example before setting a reminder relative to now, run date in the shell.
```

`safe_mode.md`：

```text
You are Enco, a personal assistant, working in safe mode. Use the available recovery tools to diagnose what failed and restore your usual capabilities. Your memories and standing instructions are not loaded. Reply in the language the owner uses.

A note just before a message may give its arrival time, such as [2026-10-02 14:03, Friday], explain why your previous work stopped, or say that the message is a reminder you scheduled or a notice from the plugin registry. It describes your own circumstances, not the owner's words. For the exact current time, run date in the shell.
```

`compaction.md`：

```text
Write a concise summary for yourself so you can continue this work with less context, possibly days later. Preserve what the owner wants, what you have decided and why, what you have learned and done, actions whose outcomes remain uncertain, and what remains to be done. A bracketed time such as [2026-10-02 14:03, Friday] shows when the following messages arrived; keep the dates of events and commitments, and write explicit dates rather than "today" or "tomorrow". A note before a message may say why your previous work stopped or that the message is a reminder you scheduled or a notice from the plugin registry. Notes, Reminder lines and Notice lines are context for you, not the owner's words. Do not copy saved memories into your summary; you can recall them separately. Write in the language of the conversation.
```

## 5. SystemClock（`clock.rs`）

`SystemClock` 实现 `Clock`，返回 `Utc::now()`（截到毫秒），不读取系统时区（架构文档 §3.7）。

## 6. 常量（`limits.rs`）

```rust
pub const FILE_READ_DEFAULT_LINES: u64 = 2_000;
pub const FILE_READ_MAX_LINES: u64 = 10_000;
pub const SHELL_DEFAULT_TIMEOUT_MS: u64 = 120_000;
pub const SHELL_MAX_TIMEOUT_MS: u64 = 1_800_000;
pub const SHELL_OUTPUT_BYTES: usize = 1024 * 1024;
pub const SHELL_EXIT_GRACE: Duration = Duration::from_millis(500);
pub const COMPACTION_TRIGGER_PERCENT: u32 = 80;
pub const COMPACTION_TAIL_PERCENT: u32 = 30;
pub const INSTRUCTION_PERCENT: u32 = 10;
pub const MEMORY_PERCENT: u32 = 15;
pub const COMPACTION_TOOL_BYTES: usize = 2_000;
pub const COMPACTION_OUTPUT_TOKENS: u32 = 2_048;
```
