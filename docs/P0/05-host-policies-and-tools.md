# 05 宿主：原生工具、上下文源与出厂 composer（enco-host）

enco-host 为内核端口提供原生适配器：`SqliteStore`（03 §3）、原生工具、`WorkspaceContextSource`、`FactoryComposer`、`SystemClock`，以及记忆（06）。这里是 P0 中所有"策略"与"排版"所在的地方。

## 1. 工作区

```text
$ENCO_HOME/workspace/        工具的默认工作目录；Agent 的"家"
  AGENTS.md                  主人的常驻指令（可选，Agent 可以编辑）
```

工作区在 P0 中是普通目录，**不初始化 git**（git 集成属于 P2）。守护进程启动时确保 `workspace/` 存在。记忆不在工作区中（06）。

路径规则（所有文件工具一致）：相对路径相对于工作区解析；允许绝对路径（当前不设沙盒）；不做 `~` 展开。

## 2. 原生工具（`tools/`）

```rust
/// 返回全部原生工具。`workspace` 用于解析相对路径和作为 shell 的默认工作目录。
pub fn native_tools(workspace: PathBuf) -> Vec<Arc<dyn Tool>>;
/// 救生集：安全模式下唯一可用的工具，也是默认 Session 要求常驻的工具。
pub const LIFELINE: [&str; 5] = ["fs_read", "fs_write", "fs_edit", "fs_list", "shell_exec"];
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

已知限制：P0 只终止 `sh` 本身，命令派生的后台孙进程可能存活；没有重定向输出的后台进程在工具返回后写管道时会收到 `SIGPIPE`。进程组的处理留到后续阶段（需要新增依赖），不要在 P0 中自行引入。

工具描述与省略原因中出现的上限（默认行数、超时、宽限时间、预算比例、记忆文本长度等）都由 `limits.rs` 的常量生成，不另写数字。

## 3. WorkspaceContextSource（`context.rs`）

实现 `ContextSource`，不使用 `ContextQuery` 中的 Event。每次调用都重新读取文件，所以对常驻指令的修改在下一个 Round 立即生效。

- `workspace/AGENTS.md` 存在时 → `Candidate { id: "workspace:AGENTS.md", kind: Instruction, text }`；不存在不是错误，只是没有候选。
- 文件不是合法 UTF-8 时返回 `ContextError`，message 指明文件。宁可让这一轮明确失败，也不要悄悄跳过主人的指令。

## 4. FactoryComposer（`composer.rs`）

出厂 composer，也就是安全模式使用的 composer。`FactoryComposer::new(workspace: PathBuf)`，`code()` 为 `CodeRef::Native { name: "factory-composer", version: crate 版本 }`。

它是纯函数：只读取 `ComposeInput` 和构造时传入的工作区路径。

### 4.1 System 消息

一条 System 消息，由以下各节依次拼接（没有内容的节整节省略）：

```text
{prompts/system.md；安全模式时改用 prompts/safe_mode.md}

## Environment
- Current time: {now，RFC 3339，带偏移} ({英文星期})
- Workspace: {工作区绝对路径}
- Session: {session.name}

## Standing instructions (AGENTS.md)
{Instruction 候选的内容}

## Memory (authoritative; overrides anything said earlier in the conversation)
- {内容} (id: {候选 id 去掉 "memory:" 前缀})
…

## Summary of earlier conversation
{transcript.summary}

## Note
The previous run ended with: {interrupted | cancelled | failed: <message> | budget exhausted}. Tool results in the history show what did and did not happen.
```

`Note` 一节只在 `previous_run_end` 存在且不是 `Completed` 时出现。

### 4.2 预算

所有估算都使用 `estimate_tokens`（03 §1.11）。

- **常驻指令**：总估算不超过 `context_tokens` 的 10% 时全部纳入，否则整份省略，记录 `Omission { source: id, reason: "instructions exceed 10% of the context window" }`。
- **记忆**：按候选的顺序（即来源给出的优先级，06 §5）逐条纳入，累计不超过 `context_tokens` 的 15%；超出的每一条都记录 `Omission { source: id, reason: "memory budget (15% of the context window) exceeded" }`。
- `plan.omitted` = `input.context.omitted`（来源报告的，原样放在最前）++ 上面两条规则产生的省略。

### 4.3 Reply 计划

```text
items = [System 消息] ++ [PlanItem::Log { pos } for item in transcript.items]
tools = input.tools 全部，原样复制（P0 不做渐进式披露，那是 P2 默认 composer 的职责）
max_output_tokens = budget.max_output_tokens
```

### 4.4 何时压缩

```text
total = est(System 消息) + Σ est(serde_json::to_string(item.message)) + est(serde_json::to_string(tools 的 spec)) + budget.max_output_tokens

if total <= context_tokens * 80%:
    返回 Plan
keep = context_tokens * 30%
b = round_ends 中最早的那个位置，使得 Σ est(pos > b 的条目) <= keep
if b 存在:
    返回 Compact { upto: b, plan: compaction_plan(b) }
elif total <= context_tokens:
    返回 Plan                                   // 超过阈值但仍然放得下
else:
    返回 Err(ContextOverflow { needed: total, window: context_tokens })
```

### 4.5 压缩计划

为了不依赖各家服务商对"历史中有工具调用但请求不带工具"的处理差异，压缩请求把待摘要的历史渲染成纯文本：

```text
items = [
  System: prompts/compaction.md,
  User:   "Previous summary:\n{transcript.summary}\n\n"（有旧摘要时）
          + "Conversation to summarize:\n" + 按顺序渲染 pos <= b 的条目：
              User 消息       → "Owner: {text}"
              Assistant 文本  → "Enco: {text}"
              工具调用        → "Enco called {name} with {arguments}"
              Tool 消息       → "Result of {name}: {content，最多 2000 字节}"
          + "\n\nWrite the summary now."
]
tools = []
max_output_tokens = min(2048, budget.max_output_tokens)
```

### 4.6 提示词（`prompts/`，用 `include_str!` 嵌入）

`system.md`：

```text
You are Enco, a personal assistant working for one owner. You run on the owner's machine with full access to the file system and the shell, so act deliberately: read before you write, and say what you are about to do before any action that is hard to undo.

Reply in the language the owner uses. Be concise.

Memory
- Memories are durable facts about the owner. Pinned memories, and memories that may be relevant to the latest message, appear in the "Memory" section below. They are authoritative: when they disagree with something said earlier in the conversation, the memories win.
- When you learn something worth keeping (a preference, a person, a commitment, an important event), save it with memory_save as one self-contained statement. Pin only facts that matter in almost every conversation.
- To correct a memory, call memory_update with its id so that the old statement is replaced. To forget one, call memory_forget.
- The Memory section shows only part of what you remember. Use memory_search when something may have been saved before.

Standing instructions from the owner live in `AGENTS.md` in the workspace. Edit it when the owner asks you to change how you work.

Tools
- A tool result marked "outcome unknown" means the action may already have happened. Check the current state before trying again.
- To set a reminder, call schedule_create with an RFC 3339 time that includes the UTC offset. The current local time is given below.
```

`safe_mode.md`：

```text
You are Enco, a personal assistant, running in safe mode. Only basic file and shell tools are available, and memory and standing instructions are not loaded. Help the owner diagnose and repair whatever led to safe mode. Reply in the language the owner uses.
```

`compaction.md`：

```text
You are summarizing a conversation between Enco, a personal assistant, and its owner, so that the conversation can continue with less context. Write a concise summary that keeps: the owner's current goals and requests; decisions made and why; facts learned about the task; actions taken and their results, including any whose outcome is unknown; and open questions or unfinished work. Do not restate saved memories; they are kept separately. Write in the language of the conversation.
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
pub const COMPACTION_RESULT_BYTES: usize = 2_000;
pub const COMPACTION_OUTPUT_TOKENS: u32 = 2_048;
```
