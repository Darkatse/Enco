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

- `InstructionsContextSource::new(path)`，`path` 为 `$ENCO_HOME/AGENTS.md`。文件存在时 → `Candidate { id: "instructions:AGENTS.md", kind: Instruction, text }`；不存在不是错误，只是没有候选。
- 文件不是合法 UTF-8 时返回 `ContextError`，message 指明文件。宁可让这一轮明确失败，也不要悄悄跳过主人的指令。

## 4. FactoryComposer（`composer.rs`）

出厂 composer，也就是安全模式使用的 composer。`FactoryComposer::new(workspace: PathBuf, instructions: PathBuf)`，`code()` 为 `CodeRef::Native { name: "factory-composer", version: crate 版本 }`。

它是纯函数：只读取 `ComposeInput` 和构造时传入的工作区、指令文件路径。

### 4.1 请求的排列

请求按变化频率排列：越稳定的内容越靠前，只属于这个 Round 的内容放在最后。服务商的前缀缓存只命中与之前请求相同的开头部分。这样排列后，后一次请求以前一次请求去掉末尾上下文消息后的全部内容开头；前缀只在它包含的内容变化时改变：常驻指令、摘要、profile、可用工具、安全模式，以及时间标记所用的 UTC 偏移。实际命中多少记在 `AttemptSettled` 的 `usage.cached_input_tokens` 中。

回复请求由三部分组成：

1. **System 消息**：由以下各节依次拼接，没有内容的节整节省略。

```text
{prompts/system.md；安全模式时改用 prompts/safe_mode.md}

## Environment
- Workspace: {工作区绝对路径}
- Standing instructions: {AGENTS.md 的绝对路径}
- Session: {session.name}

## Standing instructions (AGENTS.md)
{Instruction 候选的内容}

## Your summary of the earlier conversation
{transcript.summary}
```

2. **历史**：Transcript 的条目，来自 Inbox 的输入在需要时带时间标记（§4.3）。

3. **末尾的上下文消息**：一条 User 消息，各部分之间空一行，没有内容的部分省略。

```text
[context]
Current time: {now，RFC 3339，精确到秒，带偏移} ({英文星期})

Your memories (authoritative; they override anything said earlier in the conversation):
- {内容} (id: {候选 id 去掉 "memory:" 前缀})
…

{Your previous work was interrupted before it finished. | Your previous work was cancelled before it finished. | Your previous work stopped after an error: <message> | Your previous work reached the step limit before it finished.}
The tool results above show what you did and which actions remain uncertain.
```

最后一部分只在 `previous_run_end` 存在且不是 `Completed` 时出现，四种说法依次对应 `Interrupted`、`Cancelled`、`Failed`、`BudgetExhausted`。

末尾的上下文消息不是主人说的话，所以和提醒、插件回退通知一样，是以方括号标签开头的 User 消息（03 §1.4）。采用 User 角色，让出厂策略不依赖服务商对会话中途 System 消息的支持，也避免上下文被聊天模板合并到开头而破坏前缀。它和时间标记都是计划中的内联消息，随 `AttemptStarted` 记录；它们不是 Transcript 条目，自动召回的记忆块不作为压缩输入；对话和工具结果仍可能包含记忆内容。

### 4.2 预算

所有估算都使用 `estimate_tokens`（03 §1.11）。回复的预算是 `profile.reply.budget`，压缩的预算是 `profile.compaction.budget`（12 §5）；本节与 §4.3、§4.4 中的 `context_tokens`、`max_output_tokens` 都指回复预算。

- **常驻指令**：总估算不超过 `context_tokens` 的 10% 时全部纳入，否则整份省略，记录 `Omission { source: id, reason: "instructions exceed 10% of the context window" }`。
- **记忆**：按候选的顺序（即来源给出的优先级，06 §5）逐条纳入，累计不超过 `context_tokens` 的 15%；超出的每一条都记录 `Omission { source: id, reason: "memory budget (15% of the context window) exceeded" }`。
- `plan.omitted` = `input.context.omitted`（来源报告的，原样放在最前）++ 上面两条规则产生的省略。

### 4.3 Reply 计划

```text
items = [System 消息]
     ++ 对 transcript.items 中的每一项依次放入：需要时间标记时先放 Message { User, 标记 }，再放 Log { pos }
     ++ [末尾的上下文消息]
tools = input.tools 全部，原样复制（不做渐进式披露，那是 P2 默认 composer 的职责）
max_output_tokens = profile.reply.budget.max_output_tokens
```

**时间标记。** 带 `received_at` 的条目是来自 Inbox 的输入（04 §2），接收时间按 `now` 的 UTC 偏移换算为本地时间。一条输入如果是 Transcript 中的第一条输入，或者与上一条输入不在同一个钟点（日期与小时不全相同），前面就放一条标记：

```text
[{YYYY-MM-DD HH:MM}, {英文星期}]
```

- 相隔一小时以上的两条输入必然跨过整点，所以这一条规则既标出隔了很久才来的消息，也在持续的对话中大约每小时标一次。没有标记的输入与它前面最近的一条输入在同一个钟点。
- 标记只取决于已记录的接收时间和当前的 UTC 偏移，每个 Round 渲染出的结果相同，不破坏前缀。偏移改变时（例如夏令时切换），所有标记按新偏移重新渲染一次。
- 当前时间只出现在末尾的上下文消息中。
- 显示哪些时间、怎样显示是 composer 的策略，所以规范消息（03 §1.4）不含时间。例如角色扮演的 composer 可以改用故事中的时间。

### 4.4 何时压缩

```text
total = est(System 消息) + Σ est(历史条目) + est(末尾的上下文消息) + est(serde_json::to_string(tools 的 spec)) + max_output_tokens
est(历史条目) = est(serde_json::to_string(item.message))，带时间标记时再加上 est(标记)

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

压缩边界按 `round_ends` 从早到晚选：首选最早一个使 `Σ est(pos > b 的历史条目) <= keep` 的位置；如果摘要到那里放不进压缩窗口（§4.5），就选仍然放得下的最晚位置，先压缩一部分。每份摘要都带着上一份，所以部分压缩也是进展，同一个 Round 还有压缩次数时会接着压缩。连第一个 Round 都放不进压缩窗口时，没有边界。

### 4.5 压缩计划

为了不依赖各家服务商对"历史中有工具调用但请求不带工具"的处理差异，压缩请求把待摘要的历史渲染成纯文本：

```text
items = [
  System: prompts/compaction.md,
  User:   "Previous summary:\n{transcript.summary}\n\n"（有旧摘要时）
          + "Conversation to summarize:\n" + 按顺序渲染 pos <= b 的条目：
              时间标记        → "{标记}"，规则与 §4.3 相同，放在它标注的条目之前
              User 消息       → "Owner: {text}"
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
- The final [context] message gives you the current time, some of your memories, and why your previous work stopped if it did not finish. Use it to continue the conversation and work above it; it is not a new request from the owner.
- A bracketed time such as [2026-10-02 14:03, Friday] marks when the next incoming message arrived. The first incoming message shown has one, and so does each one that arrives in a new hour; an unmarked message arrived in the same hour as the one before it.

Memory
- Your memories are durable facts about the owner. The [context] message shows pinned memories and memories that may be relevant to the latest message. They are authoritative: when they disagree with something said earlier in the conversation, the memories win.
- When you learn something worth keeping (a preference, a person, a commitment, an important event), save it with memory_save as one self-contained statement. Pin only facts that matter in almost every conversation.
- To correct a memory, call memory_update with its id so that the old statement is replaced. To forget one, call memory_forget.
- The [context] message shows only part of what you remember. Use memory_search when something may have been saved before.

Standing instructions from the owner live in the AGENTS.md file listed under Environment. Edit it when the owner asks you to change how you work.

Tools
- A tool result marked "outcome unknown" means the action may already have happened. Check the current state before trying again.
- To set a reminder, call schedule_create with an RFC 3339 time that includes the UTC offset, written like the current time in the [context] message.
```

`safe_mode.md`：

```text
You are Enco, a personal assistant, working in safe mode. Use the available recovery tools to diagnose what failed and restore your usual capabilities. Your memories and standing instructions are not loaded. Reply in the language the owner uses.

The final [context] message gives you the current time and why your previous work stopped if it did not finish. Use it to continue your work; it is not a new request from the owner. A bracketed time such as [2026-10-02 14:03, Friday] marks when the next incoming message arrived.
```

`compaction.md`：

```text
Write a concise summary for yourself so you can continue this work with less context, possibly days later. Preserve what the owner wants, what you have decided and why, what you have learned and done, actions whose outcomes remain uncertain, and what remains to be done. A bracketed time such as [2026-10-02 14:03, Friday] shows when the following messages arrived; keep the dates of events and commitments, and write explicit dates rather than "today" or "tomorrow". Tagged notices, such as reminders and plugin rollbacks, also appear as Owner lines; they are context for you, not the owner's words. Do not copy saved memories into your summary; you can recall them separately. Write in the language of the conversation.
```

## 5. SystemClock（`clock.rs`）

`SystemClock` 实现 `Clock`，返回 `Local::now().fixed_offset()`：当前时刻与本机此刻的 UTC 偏移。

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
