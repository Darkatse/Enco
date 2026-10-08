# Provider 插件

本目录是独立 Cargo 工作区，只经 [WIT 契约](../wit/CONTRACT.md) 与宿主交互，不依赖 `enco-core` 或其他宿主 crate。

| 位置 | 职责 |
|---|---|
| `openai-compatible/src/lib.rs` | OpenAI-Compatible 入口：Chat Completions 与 embeddings |
| `deepseek/src/lib.rs` | DeepSeek 入口：只导出 completion |
| `typesafe/src/lib.rs`、`protocol.rs` | TypeSafe 入口与 System One 协议：只导出 decision |
| `provider-protocol/src/lib.rs` | 共用的类型绑定与 Chat Completions / Embeddings 转换 |
| `provider-protocol/src/http.rs` | 共用的 JSON 传输、options 合并与 HTTP 错误分类 |

改动协议先读对应入口和共享转换；宿主无需识别服务商。`settings.options` 表达线上协议的附加设置，不能覆盖已记录请求的消息、模型、工具、输出上限或开启流式响应。服务商返回的额外 Assistant 字段由内核按产生它的代际追溯身份，只回放给同一插件；插件不自报名字或身份。

```sh
cargo xtask build-factory
cargo test -p enco --test daemon --test memory
cargo xtask check
```

出厂产物嵌入 `enco`。修改插件后，将构建产物交给 `enco plugin deploy <name> <path>`；用 `enco plugin status` 查看代际，用 `enco plugin rollback <name>` 回退。

真实服务示例见 [deepseek-gemini.toml](../examples/deepseek-gemini.toml)，插件边界的规格见 [07-plugin-boundary.md](../docs/spec/07-plugin-boundary.md)。密钥来自配置指定的环境变量，插件源码和配置文件不含密钥。
