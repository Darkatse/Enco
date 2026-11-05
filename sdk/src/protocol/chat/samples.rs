//! Real Chat Completions streams, kept verbatim as the tests' samples.

/// A real stream from DeepSeek's `deepseek-v4-pro` with `include_usage`: reasoning, then one call
/// to `get_weather`, then usage on the chunk that finishes, then `[DONE]`.
pub(super) const DEEPSEEK_CALL: &str = r##"data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"role":"assistant","content":null,"reasoning_content":""},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"We"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" need"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" to"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" compare"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" weather"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" in"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" November"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" for"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Paris"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":","},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Lyon"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":","},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Nice"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Need"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" to"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" determine"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" warm"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"est"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" We"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" can"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" call"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" get"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"_"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"weather"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" for"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" each"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"?"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Tool"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" gives"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" current"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" weather"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":","},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" not"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" November"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" averages"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" User"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" asks"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" \""},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"Which"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" is"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" warm"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"est"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" in"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" November"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"\""},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" likely"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" general"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" climate"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" knowledge"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":":"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Nice"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" is"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" warm"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"est"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" in"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" November"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" among"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" French"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" cities"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Then"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" \""},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"Check"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" the"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" weather"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" of"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" the"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" warm"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"est"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" one"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"\""},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" ->"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" call"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" get"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"_"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"weather"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" for"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Nice"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Need"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" current"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" weather"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" for"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Nice"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" Let"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"'s"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":" call"},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"."},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_00_bhaNxlLUZG1eZPHqNH9f9229","type":"function","function":{"name":"get_weather","arguments":""}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\""}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"city"}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\""}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":": \""}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"Nice"}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\""}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"}"}}]},"logprobs":null,"finish_reason":null}],"usage":null}

data: {"id":"a8e3c256-dfd0-4251-b965-7801c3d14fb9","object":"chat.completion.chunk","created":1793854223,"model":"deepseek-v4-pro","system_fingerprint":"a307abda487cd1b463329ccb945ce396","choices":[{"index":0,"delta":{"content":"","reasoning_content":null},"logprobs":null,"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":377,"completion_tokens":137,"total_tokens":514,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":92},"prompt_cache_hit_tokens":0,"prompt_cache_miss_tokens":377}}

data: [DONE]

"##;
