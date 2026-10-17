# 插件

每个插件目录都是独立 Cargo 工作区，以 `../../sdk` 依赖宿主分发的 SDK，只经 [WIT 契约](../sdk/wit/CONTRACT.md) 与宿主交互，不依赖 `enco-core` 或其他宿主 crate。

| 位置 | 职责 |
|---|---|
| `openai-compatible/src/lib.rs` | OpenAI-Compatible 入口：Chat Completions 与 embeddings |
| `deepseek/src/lib.rs` | DeepSeek 入口：只导出 completion |
| `typesafe/src/lib.rs`、`protocol.rs` | TypeSafe 入口与 System One 协议：只导出 decision |
| `web/src/lib.rs`、`config.rs` | web 工具声明、配置与服务声明 |
| `web/src/fetch.rs`、`search.rs`、`http.rs` | 网页读取、搜索与 HTTP 传输 |
| `test-fixture/` | 应用与评测共用的源码夹具，参与门禁，不内嵌到宿主 |
| `../sdk/src/lib.rs`、`../sdk/src/protocol.rs` | 共用的类型绑定与 Chat Completions / Embeddings 转换 |
| `../sdk/src/http.rs` | 共用的 JSON 传输、options 合并与 HTTP 错误分类 |

首次依赖预取见根目录的[构建说明](../README.zh-CN.md#从源码构建)；`xtask` 命令离线运行。

在 Enco 仓库内用 `cargo xtask build-factory` 构建宿主自带制品（产物在 `target/factory/<name>.wasm`）。每个目录自带依赖锁、lint 表与默认 Wasm 目标，也可以进入目录直接运行 `cargo build`。

修改运行中节点的插件时，把整个目录复制到 `$ENCO_HOME/plugins/<name>/`，改好源码与配置，再经 `self_apply` 或 `enco apply` 一起登记。首次构建写出 README 生成区，区外文字保留。源码以目录自己的 `.gitignore` 为准，骨架与自带插件的 `.gitignore` 列出了 `target/` 与 `.DS_Store`，Enco 本身不另设默认规则；上级目录与 Git 的规则不参与。每次接纳把源码与配置存入 Enco 的内容库，经 `enco history` 查看，经 `enco rewind <登记编号>` 倒带文件后再登记。删除出厂插件的覆盖目录并登记，会恢复出厂代码。改名使用 `plugin_rename` 或 `enco plugin rename <name> <to>`，以保留身份。

`enco plugin deploy <name> <path>` 供主人与评测运行器直接登记没有源码的制品，不推进源码意图。

## 配置

```toml
[plugin.web.config]
search = ["exa", "tinyfish"]
# urls = { jina = "http://reader.internal", exa = "http://search.internal" }

[plugin.web.secrets]
exa = "EXA_KEY"
tinyfish = "TINYFISH_KEY"
# jina = "JINA_KEY"
```

`config` 是插件配置；`secrets` 把凭证名映射到环境变量名，不填写密钥值。

工具行为以当前代际的工具描述为准；当前三层状态与不可用原因通过 `self_status` 查看，登记历史通过 `self_history` 查看。开发契约见 [CONTRACT.md](../sdk/wit/CONTRACT.md)，模型服务配置示例见 [deepseek-gemini.toml](../examples/deepseek-gemini.toml)。
