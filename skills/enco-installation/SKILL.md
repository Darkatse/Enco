---
name: enco-installation
description: "帮助用户安装、配置和使用 Enco，完成源码构建、home 初始化、模型与记忆配置、Telegram 与 Discord 渠道接入以及运行验证。面向用户侧部署与日常操作。"
---

# Enco 安装与使用

协助用户在选定的设备上构建、配置并运行 Enco。使用用户的语言说明操作及结果，沿用用户已经选定的目录、模型服务商、环境变量名与渠道设置。

## 确认部署范围

先从用户说明或现有环境中确认以下信息，只补问影响当前步骤的缺失项：

| 事项 | 说明 |
|---|---|
| 主目录 | 默认 `~/.enco`，可通过环境变量 `ENCO_HOME` 改用其他路径 |
| 模型服务 | 回复与压缩使用的服务商（`deepseek`、`gemini` 或 `openai-compatible`）、模型名、窗口与输出上限，以及对应的 API Key 环境变量名 |
| 记忆召回 | `[memory] embedding` 使用的向量模型 endpoint；是否启用可选的 `typesafe` 相关性过滤 |
| 接入方式 | 仅使用本地 CLI（`enco chat` / `enco send`），还是同时接入 Telegram 或 Discord（私聊或群聊） |

已有 `~/.enco` 时先执行 `enco status` 或检查现有文件。数据库模式版本 14 需要新库或由主人迁移旧库，Enco 不自动迁移已有数据；遇到旧库版本报错时，先向用户说明并确认备份或重建方案，不要直接删除 `~/.enco/.data/`。

## 构建与初始化

Enco 目前从源码构建（后续发布预编译版本后将提供一键安装脚本）。仓库根目录的 `rust-toolchain.toml` 固定了所需的编译器版本（Rust 1.99.0）与 `wasm32-wasip2` 目标，`rustup` 会在首次调用时自动安装。

1. 在仓库根目录预取依赖并构建内置出厂插件，然后初始化主目录：

```bash
cargo fetch
for manifest in sdk/Cargo.toml evals/runner/Cargo.toml plugins/*/Cargo.toml; do
    cargo fetch --manifest-path "$manifest"
done
cargo xtask build-factory
cargo run -p enco -- init
```

2. `enco init` 不会覆盖已有的 `config.toml`，并会将宿主自带的 SDK 写入 `$ENCO_HOME/sdk/`。如果系统未能自动识别 IANA 时区，在 `$ENCO_HOME/config.toml` 的 `[owner]` 中补上 `timezone`（例如 `"Asia/Shanghai"`）。

## 配置模型、记忆与常驻指令

参照仓库中的 [examples/deepseek-gemini.toml](../../examples/deepseek-gemini.toml) 编辑 `$ENCO_HOME/config.toml`：

- 配置文件中永远只填写环境变量名（如 `api_key_env = "DEEPSEEK_KEY"`），不要把密钥明文写进 `config.toml`。引导用户在启动 `enco serve` 的环境或服务管理器中导出对应的环境变量。
- 需要向模型发送图片时，在对应的补全 endpoint 下添加 `accepts = ["image"]`。
- `[profile.default]` 指定 `reply` 与 `compaction` 所用的 endpoint 列表；需要自定义上下文组装时可加 `composer = { plugin = "<名字>", options = {} }`，不写则默认使用出厂 `composer`。
- 主人的常驻说明写在 `$ENCO_HOME/AGENTS.md` 中，每轮对话开始时由宿主读取。
- 需要配置网页搜索与读取插件时，参照 [plugins/README.md](../../plugins/README.md) 添加 `[plugin.web.config]` 与 `[plugin.web.secrets]`。

## 接入 Telegram 与 Discord

需要接入聊天渠道时，按 [使用与渠道指南](../../docs/Usage.md) 填写配置并准备平台权限：

- Telegram：配置 `[channel.telegram]`，将用户的数字 ID 作为字符串填入 `owner`，在 `secrets = { token = "TELEGRAM_BOT_TOKEN" }` 中绑定环境变量。
- Discord：配置 `[channel.discord]`，将用户的 Discord ID 字符串填入 `owner`，绑定 `secrets = { token = "DISCORD_BOT_TOKEN" }`。提醒用户在 [Discord 开发者后台](https://discord.com/developers/applications)的 Bot 页面开启 **Message Content Intent**，使用 `bot` 与 `applications.commands` 作用域邀请 bot（私聊需与用户同在一个服务器），并仅在需要服务的频道授予查看频道、发送消息、在线程中发送消息、读取消息历史、附加文件与嵌入链接权限。
- 群聊与服务器频道隔离：仅当连接配置了 `group_profile` 时才服务群聊。该 profile 必须设置 `owner_context = false`、`requires_lifeline = false`，并显式列出不含记忆、文件与 shell 工具的安全白名单（如 `tools = ["web_search", "web_fetch"]`）。

## 启动、变更登记与验证

1. 启动后台服务并检查状态：

```bash
cargo run -p enco -- serve
```

在另一个终端验证运行状态：

```bash
cargo run -p enco -- status
```

2. 服务首次启动会自动登记初始配置与出厂插件。后续只要修改了 `$ENCO_HOME/config.toml` 或 `$ENCO_HOME/plugins/` 下的源码，必须运行 `enco apply --reason "说明修改"` 才能生效；重启不会读取未登记的修改。登记配置时会对新增的使用发起一次最小真实调用，若因缺少环境变量、网络不通或模型参数不匹配而拒绝登记，按报错修复后重新 `apply`。
3. 核对 `enco status` 的输出，确认配置与各插件的运行中、已登记与 home 三层一致，且已启用的渠道连接处于正常状态。
4. 简要向用户汇总 `$ENCO_HOME` 位置、已生效的 endpoint 与渠道、服务启动方式，以及常用入口（`enco chat`、`enco send --image`、`enco apply`、`enco rollback`）。
