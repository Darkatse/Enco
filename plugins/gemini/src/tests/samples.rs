//! Real `streamGenerateContent` streams from `gemini-3.5-flash-lite`, kept verbatim as the tests'
//! samples.

/// A signed call to `get_weather`, then a part of empty text that finishes the reply.
pub(super) const SIGNED_CALL: &str = r##"data: {"candidates": [{"content": {"parts": [{"functionCall": {"name": "get_weather","args": {"city": "Nice"},"id": "call_102324"},"thoughtSignature": "EmAKXgFpFH0TXVjT8mewzmI5mmtURJy5kcRnKjwjfBYM4ltzm1ez06zf2TqK4MjZhUDQDzzI/KLHGsi3Aa+KKzowBYn5Y5stU4vwZu+ibsiPe8Pfu7Vtjy+Zm8YXz9BdDoM="}],"role": "model"},"index": 0}],"usageMetadata": {"promptTokenCount": 68,"candidatesTokenCount": 16,"totalTokenCount": 84,"promptTokensDetails": [{"modality": "TEXT","tokenCount": 68}],"serviceTier": "standard"},"modelVersion": "gemini-3.5-flash-lite","responseId": "FAvsaqzFHcWG1sgP88HPkQQ"}

data: {"candidates": [{"content": {"parts": [{"text": ""}],"role": "model"},"finishReason": "STOP","index": 0}],"usageMetadata": {"promptTokenCount": 68,"candidatesTokenCount": 16,"totalTokenCount": 84,"promptTokensDetails": [{"modality": "TEXT","tokenCount": 68}],"serviceTier": "standard"},"modelVersion": "gemini-3.5-flash-lite","responseId": "FAvsaqzFHcWG1sgP88HPkQQ"}

"##;

/// Text in two chunks, then the signature on a part of empty text.
pub(super) const TEXT: &str = r##"data: {"candidates": [{"content": {"parts": [{"text": "Hello,"}],"role": "model"},"index": 0}],"usageMetadata": {"promptTokenCount": 7,"candidatesTokenCount": 2,"totalTokenCount": 9,"promptTokensDetails": [{"modality": "TEXT","tokenCount": 7}],"serviceTier": "standard"},"modelVersion": "gemini-3.5-flash-lite","responseId": "FQvsary4B87nwbUPhvTu2AM"}

data: {"candidates": [{"content": {"parts": [{"text": " how are you today?"}],"role": "model"},"index": 0}],"usageMetadata": {"promptTokenCount": 7,"candidatesTokenCount": 7,"totalTokenCount": 14,"promptTokensDetails": [{"modality": "TEXT","tokenCount": 7}],"serviceTier": "standard"},"modelVersion": "gemini-3.5-flash-lite","responseId": "FQvsary4B87nwbUPhvTu2AM"}

data: {"candidates": [{"content": {"parts": [{"text": "","thoughtSignature": "EmAKXgFpFH0T+pBDKHYZOesDVEwBnpPIl1I80qzzVSq5vfRnnuWjf0cVka+TTjsGZni6HFV2YZJk+yqJX7ebrInXB5Z2qIaAuGw0zGOe9O6RctVRv3wW5wwM9EF45o7Xpgg="}],"role": "model"},"finishReason": "STOP","index": 0}],"usageMetadata": {"promptTokenCount": 7,"candidatesTokenCount": 7,"totalTokenCount": 14,"promptTokensDetails": [{"modality": "TEXT","tokenCount": 7}],"serviceTier": "standard"},"modelVersion": "gemini-3.5-flash-lite","responseId": "FQvsary4B87nwbUPhvTu2AM"}

"##;
