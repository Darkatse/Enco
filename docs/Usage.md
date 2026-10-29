# 使用与渠道指南

本文档说明 Enco 的日常命令、多模态输入输出、对话回溯、Telegram、Discord 与 QQ 渠道接入，以及配置和插件的登记与回退。配置字段的完整示例见 [examples/deepseek-gemini.toml](../examples/deepseek-gemini.toml)，插件目录结构见 [plugins/README.md](../plugins/README.md)。

## 目录与常用命令

默认主目录为 `~/.enco`（可通过环境变量 `ENCO_HOME` 修改）：

- `config.toml`：模型 endpoint、profile、记忆与渠道配置
- `AGENTS.md`：主人的常驻说明，每个 Round 开始时读取
- `plugins/<名字>/`：本地源码插件目录
- `sdk/`：随宿主分发的插件 SDK
- `manual/`：由宿主根据当前运行代际自动生成的只读手册
- `workspace/`：默认工作区
- `.data/`：Enco 的持久库（SQLite、内容寻址 blob、制品与已登记意图）。数据库模式版本 16 需要新库或由主人迁移旧库，Enco 不自动迁移已有数据。

### CLI 命令

| 用途 | 命令 |
|---|---|
| 初始化主目录与 SDK | `enco init` |
| 启动后台守护进程 | `enco serve` |
| 进入交互式终端对话 | `enco chat [--session <name>]` |
| 发送单条消息或图片 | `enco send [--session <name>] [--wait] [--image <path> ...] [text]` |
| 查看运行中、已登记与 home 三层状态 | `enco status [--json]` |
| 构建并登记改动过的配置与插件源码 | `enco apply [--reason "..."] [--json]` |
| 查看登记历史或对比差异 | `enco history [--from <point>] [--to <point>] [--json]` |
| 将 home 中的文件还原到某次登记 | `enco rewind <registration> [config.toml \| plugins/<name> ...]` |
| 机械回退最近一次登记中仍在生效的部分 | `enco rollback` |
| 创建工具插件骨架或导出出厂插件源码 | `enco plugin scaffold <name>` |
| 改名已登记的插件并保留身份与历史 | `enco plugin rename <name> <to> [--reason "..."]` |
| 查看会话列表、日志或检视某次模型请求 | `enco sessions`、`enco log`、`enco inspect [--attempt <id>]` |
| 将对话回溯到指定日志位置 | `enco backtrack [--session <name>] <epoch>:<seq>` |
| 切换会话 profile 或安全模式 | `enco profile <session> <profile>`、`enco safe-mode <on\|off>` |
| 查看或取消定时任务、遗忘记忆 | `enco schedules [--cancel <id>]`、`enco memory [--forget <id>]` |

## 会话、多模态与回溯

### Profile 与上下文组装

每个会话使用的模型与组装策略都在 profile 中定义。默认使用宿主自带的 `composer`（空选项），也可以在 profile 中指定 `composer = { plugin = "<名字>", options = {} }`。composer 会声明自己的选项 schema，登记配置时据此校验每个 profile；它只接收已记录的历史、常驻说明与召回的记忆，没有文件或网络权限。安全模式下固定使用二进制内嵌的出厂 composer 与空选项。

### 图片输入与文件发送

补全 endpoint 默认只接受文本，加上 `accepts = ["image"]` 后可开启图片输入。`enco send --image <path>` 支持发送一张或多张 PNG、JPEG、WebP 图片（单张上限 10 MB，单次请求合计上限 32 MB），也可以不附文字单独发送；Agent 还能用 `fs_read` 读取这些图片。

执行任务期间，Agent 可以通过 `message_send` 主动向你汇报进度或发送文件（支持 Markdown `text`、上限 50 MB 的单个 `file` 路径，或两者一起发），发送目标跟随当前会话的回复去向。Telegram 会直接展示受支持的媒体或发送原文件，`enco chat` 则打印本地文件路径；投递失败会在 `self_status` 中报告。

### 对话回溯

聊歪了或者想换个方向重来时，可以在 Telegram 里回复那条消息发送 `/backtrack`，在 Discord 消息右键菜单里点击 **Backtrack here**，或者先用 `enco log` 查到条目的 `pos` 再执行 `enco backtrack <epoch>:<seq>`。对话会直接从那条消息往后接；如果退回的是你自己的消息，Agent 会重新回答一次。后面的消息仍留在日志里，随时可以再跳回去。回溯只移动对话位置，不会撤销文件、记忆、定时或插件的修改，系统也会把这一点明确告诉 Agent。

## 接入 Telegram、Discord 与 QQ

出厂的 `telegram`、`discord` 与 `onebot` 插件以纯函数映射器运行，宿主持有 HTTP 与 WebSocket 连接、定时器及凭证。

### Telegram

取消配置示例中 `[channel.telegram]` 段的注释，把 `owner` 设为你自己的用户 ID 字符串，并导出 `secrets` 指定的 token 环境变量。照片与图片文件会连同说明文字进入同一条消息，不支持的附件也会如实告知 Agent。私聊支持 `/session` 切换会话、`/cancel` 停止当前运行、回复消息发送 `/backtrack` 回溯，命令菜单与正在输入的提示会自动注册。

### Discord

取消示例配置中 `[channel.discord]` 段的注释：

1. 在 [Discord 开发者后台](https://discord.com/developers/applications)创建应用与 bot，在 Bot 页面打开 **Message Content Intent**，并导出 `DISCORD_BOT_TOKEN`。
2. 在 Discord 设置中启用开发者模式，复制你的用户 ID 填入 `owner`。
3. 邀请 bot 时勾选 `bot` 与 `applications.commands` 作用域，为它要服务的频道开启查看频道、发送消息、在线程中发送消息、读取消息历史、附加文件与嵌入链接权限。

私聊要求你与 bot 至少同在一个服务器里；bot 能读到的频道消息会进入映射记录，只开放必要的频道即可。Discord 使用原生 `/session`（仅私聊）、`/cancel`、`/everyone`（仅服务器）斜杠命令与消息右键菜单中的 **Backtrack here**，命令与按钮回执只有操作者自己可见，在普通消息里手打斜杠命令不会触发它。

### QQ（OneBot v11）

QQ 个人号通过主人自行部署的 [SnowLuma](https://github.com/SnowLuma/SnowLuma) 接入。此接入方式非腾讯官方支持，请使用小号，账号可能被风控或封禁。

1. 部署 SnowLuma 并在 QQ 客户端扫码登录小号。在 SnowLuma 的 HTTP 服务上打开 `enableWebSocket`，设置 `accessToken`，将 `messageFormat` 设为 `array`；HTTP 动作和 `/event` WebSocket 使用同一个端口。
2. 在启动 Enco 的环境中导出 `ONEBOT_ACCESS_TOKEN`，值与该 HTTP 服务的 `accessToken` 相同。
3. 配置连接，`owner` 填主人的 QQ 号：

```toml
[channel.qq]
plugin = "onebot"
base_url = "http://127.0.0.1:3000"
owner = "123456789"
secrets = { access_token = "ONEBOT_ACCESS_TOKEN" }
context = 0
# group_profile = "group"
```

`base_url` 使用实际启用 WebSocket 的 HTTP 服务地址。SnowLuma 与 Enco 部署在同一台机器时，只监听本机地址。启用群聊时，按下文配置独立的 `group_profile`。

私聊使用文字命令 `/session`、`/cancel`、`/backtrack`；群里发送 `/everyone on|off` 开关成员提问。引用一条消息并发送 `/backtrack` 即可回溯。普通群消息以 @ bot 为准；QQ 引用回复通常附带 @，手动删除它之后就不触发提问。其他人的命令静默忽略，带附件的文字按普通消息处理。

回复以 Markdown 原文发送，长文字分段，图片随文字发送，其他文件单独上传。发送节奏由 `onebot` 插件给出，初始最短间隔 1.5 秒，再增加至多 1.5 秒随机延迟。Run 进行时，手机 QQ 私聊显示可能短暂间断的「正在输入」；Mac QQ 与群聊不显示，取消使用 `/cancel`。

Enco 停止或重连期间发来的 QQ 消息需要重发。SnowLuma 尚未就绪或 QQ 掉线后重新登录时，渠道会自动重试连接；最近的打开失败可在 `enco status` 查看。

### 回复与转发的内容

Telegram、Discord 与 QQ 的回复和转发会连同提问记录。Agent 看到原发送者、协议给出的时间、正文和图片；只有引用或转发、没有附言的消息也能作为输入。引用内容作为对话材料呈现，其中的 `/cancel` 等文字不会成为转发者的命令。

Telegram 与 Discord 使用消息里自带的引用内容。QQ 会读取 SnowLuma 保存的原消息与合并转发，嵌套转发在一次读取的请求范围内展开，其余内容注明未展开；部署前没有保存的消息可能取不到。Discord 已删除或没有随事件提供的引用同样会写明原因。取不到引用或某一部分内容格式不符时，其余输入照常处理。

群里的触发规则保持不变：提问须按该渠道的方式发给 bot。Telegram 转发中的 @ 不算对 bot 的提及；群里的纯转发作为前文，私聊里作为输入。前文中的引用只带协议已经给出的内容，不另外请求图片或历史消息。Telegram 的局部引用与跨对话回复尚未接入。

提及尽量显示名字，提问开头用于呼叫 bot 的 @ 会省去，所以只 @ bot、没有文字、图片或引用的消息不会触发回复；被引用的消息保留其中的 @。QQ 的提及仍用号码，系统表情按随插件保存的表显示名字，未收录的保留编号。

### 实时进度卡

任务运行期间，Telegram 与 Discord 都会挂出 `Working` 进度卡，实时显示耗时，以及最近五个工具的名字、参数摘要与结局，并带一个只有主人能点的 `Cancel` 按钮，同时显示「正在输入」；最终回复送达后卡片自动删除。渠道状态以及最近失败或未知的投递都可以在 `enco status` 中查看。

### 群聊与服务器频道隔离

如果想把 bot 放进群聊或服务器频道，在连接上配置 `group_profile`，每个群、频道或线程就会分配到独立的 Session；不配则只响应私聊。

为了把群聊机器人与私人助理隔开，`group_profile` 必须设置 `owner_context = false`，并显式列出不含记忆工具的工具白名单（文件与 shell 工具仍可能读到机器上的私人文件，只开放结果适合在群里分享的工具即可；示例配置使用网页工具并设 `requires_lifeline = false`）。

默认只有主人对 bot 说话才会驱动群会话，发送 `/everyone on` 可向其他群成员开放提问，`/everyone off` 恢复默认，成员提问会保留各自的身份。设置 `context = N` 会在提问时附带最多 N 条前文消息作为引用，设为零则不带（Telegram 需在 BotFather 中关闭隐私模式或把 bot 设为群管理员才能收到普通群消息；目前暂不支持论坛话题，回复会进入 General 话题）。

## 开发与管理插件

每个插件放在 `plugins/<名字>/` 下，是以 `../../sdk` 为路径依赖的独立 Cargo 工作区，由目录自己的 `.gitignore` 决定哪些文件计入源码，不需要 Git。运行 `enco plugin scaffold <名字>` 可以生成新的工具插件骨架；如果传入的是 `composer` 这类出厂插件名，则会导出它随宿主分发的源码，从下一次 apply 起用这个目录接管出厂代码，删掉目录再 apply 就回到出厂版本。

改好插件源码或 `config.toml` 后，执行 `enco apply --reason "说明修改意图"`，把所有发生变化的意图一次性构建并登记入库（主人可以省略理由，Agent 调用 `self_apply` 时必填）。重启服务会直接恢复已登记的状态。

已接纳的源码与配置保存在 Enco 自己的内容库里。`enco history` 列出历次登记，`enco history --to <登记编号>` 查看单次改动，`enco history --to home` 查看手头还没登记的修改。想把文件退回某次登记时的状态，运行 `enco rewind <登记编号> [config.toml | plugins/<名字> ...]`，核对文件后再 apply。不指定路径时只还原那次登记之后改过的单元，且拒绝覆盖未登记的修改；显式写出路径则允许覆盖（被忽略的文件始终保留，含忽略文件的目录不会被删除，插件也保持当前名字）。Agent 通过 `self_history` 与 `self_rewind` 使用同一入口。

如果只是线上出了问题想立刻恢复运行状态，`enco rollback` 可以按登记把仍在生效的部分直接退回去，不需要重新走 Cargo 构建或模型调用。
