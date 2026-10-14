# 插件

本目录是独立 Cargo 工作区，只经 [WIT 契约](../wit/CONTRACT.md) 与宿主交互，不依赖 `enco-core` 或其他宿主 crate。

| 位置 | 职责 |
|---|---|
| `openai-compatible/src/lib.rs` | OpenAI-Compatible 入口：Chat Completions 与 embeddings |
| `deepseek/src/lib.rs` | DeepSeek 入口：只导出 completion |
| `typesafe/src/lib.rs`、`protocol.rs` | TypeSafe 入口与 System One 协议：只导出 decision |
| `web/src/lib.rs`、`config.rs` | web 工具声明、配置与服务声明 |
| `web/src/fetch.rs`、`search.rs`、`http.rs` | 网页读取、搜索与 HTTP 传输 |
| `provider-protocol/src/lib.rs` | 共用的类型绑定与 Chat Completions / Embeddings 转换 |
| `provider-protocol/src/http.rs` | 共用的 JSON 传输、options 合并与 HTTP 错误分类 |

改动插件后用 `cargo xtask build-factory` 构建（产物在 `target/factory/<name>.wasm`），再经 `plugin_deploy` 工具或 `enco plugin deploy <name> <path>` 部署。

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

工具行为以当前代际的工具描述为准；已登记的设置、不可用原因和历史通过 `plugin_status` 查看。开发契约见 [CONTRACT.md](../wit/CONTRACT.md)，模型服务配置示例见 [deepseek-gemini.toml](../examples/deepseek-gemini.toml)。
