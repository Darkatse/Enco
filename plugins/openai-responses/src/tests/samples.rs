//! Real Responses replies, kept verbatim as the tests' samples.

/// A real reply from `gpt-5.4-mini` at medium reasoning effort, with `store: false` and the
/// encrypted reasoning included: a reasoning item, then a call.
pub(super) const REASONING_REPLY: &str = r#"{
  "id": "resp_07e033f4dd0e65d9016aeb9b9103f087d2a46311e7e7a77211",
  "object": "response",
  "created_at": 1793825681,
  "status": "completed",
  "access_programs": null,
  "background": false,
  "billing": {
    "payer": "developer"
  },
  "completed_at": 1793825682,
  "error": null,
  "frequency_penalty": 0.0,
  "image": {
    "context_compression": "disabled"
  },
  "incomplete_details": null,
  "instructions": null,
  "max_output_tokens": 2000,
  "max_tool_calls": null,
  "model": "gpt-5.4-mini-2026-03-17",
  "moderation": null,
  "output": [
    {
      "id": "rs_07e033f4dd0e65d9016aeb9b91614487d28aac8f9d568756fe",
      "type": "reasoning",
      "content": [],
      "encrypted_content": "gAAAAABq65uTaVJuPGys55IzRVSTAqw-oyeAPSABhSbZzOrZUAqSpFctjj9W1fapRwhDyO5bfFUFKJetGxAHQF7NPWQJV3gvZxUgE3q_nk9nhNR7aigjfUOv1Ikzbe6bfT2VXlf8gkjqWavWwn3IQTismMS9j1XMPySvXZoZCL4LpiQ9oqE8YRlj6KNULdeshQo6YYVZr8OX2Mw_HgxCo8r15Bv8Hs0IlqvSZoH0hjgBKaqlWU-kps7W_PkTY-jxoU2mSjkUa5Q0kFISB8Ntg87q8iur2ROgGvwAhc42sRlk9oMqp5__UMLMpaqRNPL9HpYKthc34Ml3sPUVbkQOA0n3I4hQAPk8ZD3lQQU7ubmxTgWtZc5HkLLzOaPGhwk15aEKfJcduyD_U_4k0phKNJde1G_VtCtHqG_svuXDBTLHCHRqeO-HmCDieTVTVN30L9SYPy9hsZPAVDYRck8YVYWz6BGHapTiRFBtsJ8bgRk73hCJ9KMDlmZPW7L5dkFlMq2MS9EfkC-gl6OH8cI1fN71FrwlzQB14m19FSmxctOfot2Z7WcBxDxABv2eQMlUeLb3VKTxqeRiTg9vi2pHScB0DcSZuveLIhPboPzYy9yBjsAcO67qn8YkHC8O4c3cRFvF27NkV3RIB-jZOMoFpxuzYfRjLaRJPrABsvNdGvwbHqQte0wTg_XJdlZ-_S18lrx_oNbDf90pBtScVmFj4JuxCNaysRri8RGUy-1yPrWEtKY7XpQbUSNoEcAh1sJWKm1rsnhZL_cIdrB3-ojqAY8KpJ63l2Ahgy7ozctS8zA6UHGYwx1KrRwy6jPfjBtbBTc56nhqsFdopRHofLcgE6Fz3S-apmGHfQd1M5oJKeB-jr0QU3j8gNpS271A2NfiLp4uCzaLrVI2gHuogJ-boIsAUAmOvEoPmih5d2ttxXPq8k_tZywOOOYUz2g-AVD_rEK-GyO5uESgu4qDr78TMqzqU6PiNHTHaS3_3pLPB-WcdGVQRtrEGdGbIFNrMj7O2rWMITSwP8bOhjU8MfIXXyvev-lt7IlRCyG4x3XiCpmownq1ZciGgQ-pUIbik3XOjaNnyykE2-n3duHBDK3oiKbhmow0F66VZZVuEMTV45vnnLi-d0vMvrPUELaiMR-uucWvsKry35x7rvyolTVIHiaTWewXhEsbu0ZrxM7EzBEbSqthigZvZhvU2_kPnB7N-gkMUNKLBI90K2veb3Y5wdWB0TlGz8vsNLD7j04y2I_bRTL2Rq2j3SymO7DMv_3hSZ5pxhrmh7uLrH-eGsB6_ug-lCi8GC8P9JN22nIwZLCjt-HCphcK-a0m5WVb3HQMHC2XQkrr_lGfVYNKKsvzWX5AvAHM-ZvxG9K_wxGBOqh4AMLERz7vHU5BHSKTkD5nDHQQ9m0aYsZh9xs9kxt5F1cGRCMWo7QQlJl4dsmF2JCXAozFbeGkdTx81LydfczZTyb89p_mcwC33oUmkmdHTYPXaWEJmHgLYiTGoevnsh7zlkQX0OfpWfeeOtURecK_v2rgEwQczoT6vJTtrbvpOh5aHsQ92o9dHWy1DkOFPd8pknMgqgAJCxV3MPVqswvyRl5ccxGNnJR5b-qsmEOqIIPTGm3xlgLbhUfaRotJTCPkcaIi83L8lG50lUTBM4OroXB47HyYyBXP6Q3P1vqCPt65hcUzPkm0fMIeb8O0qsCV-XdTNUkHPOIKTmyu7KzJdGHkl57ks4gzJ9waOcbaN0Mqr6GZ73gkTQXUX4wkZZjhAsx4Fu4jQKQlgmT8kk-SAHQlY0qF45tOJliXlqGfjW-TmWCy6MRXVqK9BGlyyDanX_vdxHlwhLXaJYjcF4xBDNBQGvxy_XHn74kW1BX4SHIMVwLamwQr5b2FARaO3s9ZBXALGvAfuesS72GocA6PjKX9bBtYmDcTU73I7AotRdBoQW030Rgc37hCDwHWp26jugTF09Wb0pq6j-kK_86zDSVpPb04aOwi4vtTIb9mQliv5TrR82QAz3ps0bPjBfap_z2VgV6P2Rh7x9nkAGqJddLIF42QFfTZlUfp7kzlkonkEkldO3mXWhfLfC_v3Cg-QZ2Kr9S1Q7YqQoq2poTesLTH4ShFSUIzi3E0jhuYJREdZKqvKRTwYObwVWQmNvDWGHuAcOtwDlNC77Ax1lFZIQY9RedbsgEsowJNzG6GNMOdj--pDYn37xQIUXAB3ibAYLqXEwMrSA8VIbL9CFXKmMdf2VdkplqISJ59LYsmIUBLHqszzILsm637-AKSne2sOsVaC0AAmOxSj2QUhEX1vYb8bgllOIqbzXlcM6cgM6W1VQMcHtqdbjOiC99o-9S_1SM4JWtJkuFZBlHYVqa9upQ-7Ed4jOUD",
      "summary": [
        {
          "type": "summary_text",
          "text": "**Determining warmest city**\n\nI need to identify which city among Paris, Lyon, and Nice is likely the warmest in November. Based on my understanding, Nice would probably be the warmest due to its location on the Mediterranean south coast. However, to be accurate, I need to call the weather tool for Nice. Since the tool is available, I'll go ahead and make that call to gather the precise information. Keeping this concise is important!"
        }
      ]
    },
    {
      "id": "fc_07e033f4dd0e65d9016aeb9b92e9fc87d2967cfa4f7a6a0a43",
      "type": "function_call",
      "status": "completed",
      "arguments": "{\"city\":\"Nice\"}",
      "call_id": "call_BoUa9IP2QQWlhrI0mZn6jNSe",
      "name": "get_weather"
    }
  ],
  "parallel_tool_calls": true,
  "presence_penalty": 0.0,
  "previous_response_id": null,
  "prompt_cache_key": null,
  "prompt_cache_retention": "24h",
  "reasoning": {
    "context": "current_turn",
    "effort": "medium",
    "mode": "standard",
    "summary": "detailed"
  },
  "safety_identifier": null,
  "service_tier": "default",
  "store": false,
  "temperature": 1.0,
  "text": {
    "format": {
      "type": "text"
    },
    "verbosity": "medium"
  },
  "tool_choice": "auto",
  "tool_usage": {
    "image_gen": {
      "input_tokens": 0,
      "input_tokens_details": {
        "image_tokens": 0,
        "text_tokens": 0
      },
      "output_tokens": 0,
      "output_tokens_details": {
        "image_tokens": 0,
        "text_tokens": 0
      },
      "total_tokens": 0
    },
    "web_search": {
      "num_requests": 0
    }
  },
  "tools": [
    {
      "type": "function",
      "description": "Current weather for a city.",
      "name": "get_weather",
      "output_schema": null,
      "parameters": {
        "type": "object",
        "properties": {
          "city": {
            "type": "string"
          }
        },
        "required": [
          "city"
        ],
        "additionalProperties": false
      },
      "strict": false
    }
  ],
  "top_logprobs": 0,
  "top_p": 0.98,
  "truncation": "disabled",
  "usage": {
    "input_tokens": 89,
    "input_tokens_details": {
      "cache_write_tokens": 0,
      "cached_tokens": 0
    },
    "output_tokens": 92,
    "output_tokens_details": {
      "reasoning_tokens": 72
    },
    "total_tokens": 181
  },
  "user": null,
  "metadata": {}
}"#;

/// A real reply from `gpt-5.4-mini` without reasoning: a commentary message, then a call.
pub(super) const COMMENTARY_REPLY: &str = r#"{
  "id": "resp_02730356b09e1e79016aeb9b89b60887d29373bad8bbd48665",
  "object": "response",
  "created_at": 1793825673,
  "status": "completed",
  "access_programs": null,
  "background": false,
  "billing": {
    "payer": "developer"
  },
  "completed_at": 1793825675,
  "error": null,
  "frequency_penalty": 0.0,
  "image": {
    "context_compression": "disabled"
  },
  "incomplete_details": null,
  "instructions": null,
  "max_output_tokens": 2000,
  "max_tool_calls": null,
  "model": "gpt-5.4-mini-2026-03-17",
  "moderation": null,
  "output": [
    {
      "id": "msg_02730356b09e1e79016aeb9b8ae7e087d28a435b529b6e71d1",
      "type": "message",
      "status": "completed",
      "content": [
        {
          "type": "output_text",
          "annotations": [],
          "logprobs": [],
          "text": "Nice is most likely warmest in November, so I\u2019ll check its weather."
        }
      ],
      "phase": "commentary",
      "role": "assistant"
    },
    {
      "id": "fc_02730356b09e1e79016aeb9b8b013887d2ae88daef066d5391",
      "type": "function_call",
      "status": "completed",
      "arguments": "{\"city\":\"Nice\"}",
      "call_id": "call_Sh0QSf0QoVQlHB5Rvmjz4lX0",
      "name": "get_weather"
    }
  ],
  "parallel_tool_calls": true,
  "presence_penalty": 0.0,
  "previous_response_id": null,
  "prompt_cache_key": null,
  "prompt_cache_retention": "24h",
  "reasoning": {
    "context": "current_turn",
    "effort": "none",
    "mode": "standard",
    "summary": null
  },
  "safety_identifier": null,
  "service_tier": "default",
  "store": false,
  "temperature": 1.0,
  "text": {
    "format": {
      "type": "text"
    },
    "verbosity": "medium"
  },
  "tool_choice": "auto",
  "tool_usage": {
    "image_gen": {
      "input_tokens": 0,
      "input_tokens_details": {
        "image_tokens": 0,
        "text_tokens": 0
      },
      "output_tokens": 0,
      "output_tokens_details": {
        "image_tokens": 0,
        "text_tokens": 0
      },
      "total_tokens": 0
    },
    "web_search": {
      "num_requests": 0
    }
  },
  "tools": [
    {
      "type": "function",
      "description": "Current weather for a city.",
      "name": "get_weather",
      "output_schema": null,
      "parameters": {
        "type": "object",
        "properties": {
          "city": {
            "type": "string"
          }
        },
        "required": [
          "city"
        ],
        "additionalProperties": false
      },
      "strict": false
    }
  ],
  "top_logprobs": 0,
  "top_p": 0.98,
  "truncation": "disabled",
  "usage": {
    "input_tokens": 89,
    "input_tokens_details": {
      "cache_write_tokens": 0,
      "cached_tokens": 0
    },
    "output_tokens": 41,
    "output_tokens_details": {
      "reasoning_tokens": 0
    },
    "total_tokens": 130
  },
  "user": null,
  "metadata": {}
}"#;

/// A real reply from `gpt-5.4-mini` at medium reasoning effort cut off by `max_output_tokens` 16,
/// with `store: false` and the encrypted reasoning included: only a reasoning item.
pub(super) const INCOMPLETE_REPLY: &str = r#"{
  "id": "resp_062839ed399ae943016aebcc64475c87d29dfbfccaafbe3898",
  "object": "response",
  "created_at": 1793838180,
  "status": "incomplete",
  "access_programs": null,
  "background": false,
  "billing": {
    "payer": "developer"
  },
  "completed_at": null,
  "error": null,
  "frequency_penalty": 0.0,
  "image": {
    "context_compression": "disabled"
  },
  "incomplete_details": {
    "reason": "max_output_tokens"
  },
  "instructions": null,
  "max_output_tokens": 16,
  "max_tool_calls": null,
  "model": "gpt-5.4-mini-2026-03-17",
  "moderation": null,
  "output": [
    {
      "id": "rs_062839ed399ae943016aebcc65041087d299eeb64a15884b1c",
      "type": "reasoning",
      "content": [],
      "encrypted_content": "gAAAAABq68xll_ZOGLFous8ipxs4esY4DMiwlPYQYemjM3a-Uy3DHsrBaahClFZmew4JfBc1h_Wtvn3l9vyg9R4MIp3rVfMFyfTE3CNjb8oPR8XL7bwUn5qNAl5Y_UklVX-DhW_70ubrnfeux0VqXDOQWX8AKAFPoSDZdV_GRTn3PJ4_bsCGem6hvlwPJz-vzKTNgERtQ2lCCnkOtSYJ33QE02i7wsrOoGjEBW6cCRbjL1O1LDWCmQszRpAbU79GztKXnJWTpcWZD4NlxRTPcFA3O022CSAupmPQrxb6BTUMRyNk9ERXm0CgE-UFjzns1jgVc83d-s--pXWse1zn62phaaE1d2f8DW3ZSCENwIhyFl6G1nKZ0C6ITNnXPM_lQb6z1-P1Q_tZi2KYDfpXKt62aG_tvcmTWyitlHCEx0gYYNA0rNcmaQiBqxWdGFrorcN0oQk6q2tJgbnTxCw-XbW6yw4MltnpGUnCu7993gDBAlpcDvt1qWHbVivWB68hmNOt8SkI1tG3yIrnfoX8Xb0Lim89GB0jcATr2W61Cew4PEfDmPxxkVrmmuFHqZopVgY2YcEYtLSScuY3WQ7phLj4A_m7HwslXiBCVscW4O8G6pbFXfmwJbZE_uItu4Ej44rYjpGiISOvMIo0k8dyKzAAxaVnmw1lFQTQVMS7WkXDJlb_Jp0njckdMrHWW71PAZWOj6lQmUPW4MbiacyU_lD93YSxk1XlF-MBbkpPs_Y-QMD4i2TrjSzVLMMNLHhwfu_ANbS_YnO_KUR0FWp_Te3sq4YpgWRaOF8F0qqdfGgL7Rfm4lVh3kMq4Ob1847uzj_L0AR4l5hVCoYVxmUHsYjefwGe8azAuoF7Z-FqY4VEARjpiiSStMA2dhvaNAPt-3Wc8tVbwKtXMC9S3yStHCcUwKeXT0afhRr-T9mfeMDb6fz2ps2nRpANTA17UEgDj3hEgp_NufCAWyJ6662WNVt6pAzqMkA_RLUKqQpJdtv7xNqfe1QbL3HVrmGxBrGRHs7A189cnnv92ZGvXqSTdNRV9E85HzaRArEdeAO-ib4ODgelii3c2NA3qA-UHUbxMkLCqR5kqMvgeewPgSpiv12qIZvMBuDNZlwBuTE3yl0QekdtXkvVT3FZvqoDhNk22YzxHnnOkqmrR5OByMi0pZoAIN8RWMmcJC0DbQdIKdyxybHo-A9PnYZrE5xis_CJr1ObeTM4wOVnd75mb0P_SSGftuVojfW7I5I5J9B0BHEAPDVXyyUNHgRndk0WPKvtkLxLbvcoXSmro3LxHqoiAMbI_g_HhyJ6AZYHCjMt8-8drZO17V0DQmo=",
      "summary": []
    }
  ],
  "parallel_tool_calls": true,
  "presence_penalty": 0.0,
  "previous_response_id": null,
  "prompt_cache_key": null,
  "prompt_cache_retention": "24h",
  "reasoning": {
    "context": "current_turn",
    "effort": "medium",
    "mode": "standard",
    "summary": null
  },
  "safety_identifier": null,
  "service_tier": "default",
  "store": false,
  "temperature": 1.0,
  "text": {
    "format": {
      "type": "text"
    },
    "verbosity": "medium"
  },
  "tool_choice": "auto",
  "tool_usage": {
    "image_gen": {
      "input_tokens": 0,
      "input_tokens_details": {
        "image_tokens": 0,
        "text_tokens": 0
      },
      "output_tokens": 0,
      "output_tokens_details": {
        "image_tokens": 0,
        "text_tokens": 0
      },
      "total_tokens": 0
    },
    "web_search": {
      "num_requests": 0
    }
  },
  "tools": [],
  "top_logprobs": 0,
  "top_p": 0.98,
  "truncation": "disabled",
  "usage": {
    "input_tokens": 20,
    "input_tokens_details": {
      "cache_write_tokens": 0,
      "cached_tokens": 0
    },
    "output_tokens": 16,
    "output_tokens_details": {
      "reasoning_tokens": 16
    },
    "total_tokens": 36
  },
  "user": null,
  "metadata": {}
}"#;

/// A real stream from `gpt-5.4-mini` at medium reasoning effort, with `store: false` and the
/// encrypted reasoning included: reasoning, then three parallel calls, then `response.completed`.
pub(super) const REASONING_STREAM: &str = r##"event: response.created
data: {"type":"response.created","response":{"id":"resp_09365a692b7f9e41016aec0b0bb53487d2b6638f5188d3f563","object":"response","created_at":1793854219,"status":"in_progress","access_programs":null,"background":false,"completed_at":null,"error":null,"frequency_penalty":0.0,"image":{"context_compression":"disabled"},"incomplete_details":null,"instructions":null,"max_output_tokens":2000,"max_tool_calls":null,"model":"gpt-5.4-mini-2026-03-17","moderation":null,"output":[],"parallel_tool_calls":true,"presence_penalty":0.0,"previous_response_id":null,"prompt_cache_key":null,"prompt_cache_retention":"24h","reasoning":{"context":"current_turn","effort":"medium","mode":"standard","summary":null},"safety_identifier":null,"service_tier":"auto","store":false,"temperature":1.0,"text":{"format":{"type":"text"},"verbosity":"medium"},"tool_choice":"auto","tool_usage":{"image_gen":{"input_tokens":0,"input_tokens_details":{"image_tokens":0,"text_tokens":0},"output_tokens":0,"output_tokens_details":{"image_tokens":0,"text_tokens":0},"total_tokens":0},"web_search":{"num_requests":0}},"tools":[{"type":"function","description":"Current weather for a city.","name":"get_weather","output_schema":null,"parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]},"strict":false}],"top_logprobs":0,"top_p":0.98,"truncation":"disabled","usage":null,"user":null,"metadata":{}},"sequence_number":0}

event: response.in_progress
data: {"type":"response.in_progress","response":{"id":"resp_09365a692b7f9e41016aec0b0bb53487d2b6638f5188d3f563","object":"response","created_at":1793854219,"status":"in_progress","access_programs":null,"background":false,"completed_at":null,"error":null,"frequency_penalty":0.0,"image":{"context_compression":"disabled"},"incomplete_details":null,"instructions":null,"max_output_tokens":2000,"max_tool_calls":null,"model":"gpt-5.4-mini-2026-03-17","moderation":null,"output":[],"parallel_tool_calls":true,"presence_penalty":0.0,"previous_response_id":null,"prompt_cache_key":null,"prompt_cache_retention":"24h","reasoning":{"context":"current_turn","effort":"medium","mode":"standard","summary":null},"safety_identifier":null,"service_tier":"auto","store":false,"temperature":1.0,"text":{"format":{"type":"text"},"verbosity":"medium"},"tool_choice":"auto","tool_usage":{"image_gen":{"input_tokens":0,"input_tokens_details":{"image_tokens":0,"text_tokens":0},"output_tokens":0,"output_tokens_details":{"image_tokens":0,"text_tokens":0},"total_tokens":0},"web_search":{"num_requests":0}},"tools":[{"type":"function","description":"Current weather for a city.","name":"get_weather","output_schema":null,"parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]},"strict":false}],"top_logprobs":0,"top_p":0.98,"truncation":"disabled","usage":null,"user":null,"metadata":{}},"sequence_number":1}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"rs_09365a692b7f9e41016aec0b0cb2e087d2bf859493e85fd7f9","type":"reasoning","content":[],"encrypted_content":"gAAAAABq7AsMtVqClOd4eicFXmZdC-U-gske1-68uj4S8JTj43yw-u8WEvKksZvf8cRra1oeWkfggvnJWNVqK9pzE0aOJNuWHrDft3YHVy5q7WNsDU1ZKUhhgfhLzjvWw5rGb1mjpLYwyUUJDSmfixWiaKo1bfCbA0zsGVSWZND7Uvq8ATLtZ2ejdjOZkh8HbupCbFjEl_LJwRUHrFs3eLGXljGMbgeIzBqouos-BnT2_x2xSmTuwMwGsdwkrpCabNOJk1YQ5chlW5KUfAVsGz-4CInLHzjb2NKj45T57oLjH1himbyyVJwtY9c3GTkgz77AYkNalZEEFLMecEkLHzK-p08higkMykAv85tAq6eIl2jluOnDHTeUK0mZmm8ysNhQfj7Lv7ct7wjkhE2oZxrcgPrse0wKJbGxLvAm--zqo1InMje-OwHlnSBif4nbI7HPZffbmomUmbn8LhzOUdmBdrtQMARPkIn2GpMVgYXTPp5ce3Qi3unILCiKI4tL9AQ3g3ab6Lo30nB4pEy9hPKKO8oCR4gp8ju4hbooCFeKonagVD2UIdIpp-XAcGhCd642SI46xDokIg6TCcUoZwUHzlIIrb6axb5bH1qboYNggxfC1saHN-FZSvHvvbobFnwDYvQuh5EoVnpdQOvjyjEpMkmZswdgfPKUpQSOz8KOazMOXbori_N1nMcbjYsaKZnE9A4d7FbMbdD6b1AK6r5odhu0nRyrMek96HtVpb_nxwbMYMgPTqbiUwPuRFaBOxSxXZcmu-EdPyXxkriSWuCB4FE6Htzzq-HbHdwzTzUVTePiNlwImgpGGeeaVwnGq0CGME-yEaJAlS9Zkq2bMGY6QGFxd226_Nt0FDw1haF16EjDIL7DZLgjCvJN4kuAU1HT_b8g8_9P7Lz9NilwAOd4YoIPvmQhjYwKXvgwRtGitlCt4RH30U4Gz_Koa2z7RsBcG7yi3v5MuSlA1N9EO_Lbv98kGueuhxcgWXZ2JhK_QLFQ7LHz4NeOEI1G9o5FEFFlxVy1BwcWKA47vnT23NzfJm42KwVjpWOzL8UVo6lj--09ws8MluQHm2Gmbt8_v96qxNEYeIFTn7sJFxByZvnzMlf14KzdqW9tGYafeS_JlnbBenCV2RS_VPR0L-EtSTo37p1Q-tKdsQilLoo9prP9DBtBhp62n2HXysBzxRftg7YleGkLMfKQxrmtBnbIaXwHN4YndryPMK6SCWbxV4EU6vmIHlUgJA==","summary":[]},"output_index":0,"sequence_number":2}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"rs_09365a692b7f9e41016aec0b0cb2e087d2bf859493e85fd7f9","type":"reasoning","content":[],"encrypted_content":"gAAAAABq7AsNdljD9Ahiwm7HRMlZGoAdOAZp0SpbjrFH-QP2wuc2mhQR4l3qMriOtaZ5PBTDX_xtAiC-Z-KHOlcZeos_xzNk3LqrA-MWzouHZfs5msEcNEo7ZK8bHydIxV1tmiOMolDnQht_qO5lxhBU_m902nxQoFYXkX9hYTGYPv5qYR-NlgyqPEixIfDn_WHIu_7hYfu6pyJ9Ch-niZ4YLJBpAwklGDLlLVX6FlzXbTf_AT_p6ai2PWyOtyrbeZ5qHBHDZQsBt-Uta5VgTrS0vUnEMM3yRikDI2Fefpo_Vme3jPfka5_v8FQ38mckv0NGzVRRc2bdbRnzX4vwB8wvnTgAHFMrtrnozrcVuVjKbwXBvTc3fwOx-fwDaCef03h8onyGe-7OEmdDIDpZcqOWfgT-fLK3-heqTdgdBmKgXS13XB5WkkpEXHCWblwLVlvhxH5jvxsBP5SrnOsBztp-vZqI-ectCeVWBADkiLWhyGwehjJhntPS_nZF514mLypjXk7hSapLwHChVvnPH3NivMXaymF3CC9ElbBH92kLsqaABurhOJR4jyguKmGlNRo66EW-Ws-Amfny-uidn6hzMPEUpT-D5GIM1uvE9WTIoFLeVlOOdV5J7t3pdSucU6Lc3ZnI8bTaAyke_5zFInU5dJRBmPqrbsAaLc9vJtDc-BcCd-ZbAILtQI8oTAo-LKl7E9e3rZLfdDbyGG94tllQCBTt_eMK39rQvCU0owidS76FdMjVHrcNlrD4yPW7OzGnAxy9lRoyy-6289PKm4DY3qOdIKEF_iR-wIyuursERhaN0dnC0kB8JjynfzfoL3JGmxocaeRIoi_cP7GE-e1FeN9aWQkgJhw-LgxutTYm1oVN_b_aKRKE0fKHr_wn1Bj8HkyVy829UIXYVbMa8t1U232wGrr2Li8wSUHVNZRsOxvBB5o-L4iSAJBqRITWx5e5fDf4Hk7T0XAsbouhmNcblthMjXXSD7vJmq4Uy7UQOi_rerGAv-FP3qgu-PjwzkdR-qm4oTplLkmTzythYtxy99lnmCNepbXxLlMBTq9Ik2c4TX_Ej74WqpJfnAlwQVVohBoI04H3sNy6wBZg5jGrdwrmeLcRKgV0i87ZX7KyCf3_t2ZIdSxMIwkWwoaZfcrt1ceKM0y9Dg1HopQE7mVq4V7daK3xHZ15hURxdB5aDczynGHptv0hiV0PtcHFy845DahhWlrMG0dWU6co37fW0XLx3W-tVBZ6I7bUXjTUpfLTa27IHDeZccEWocMM0ggodoKR6yHJ17EbLhGAns_CvZstV09GqvFm1WQXPUhAMSJ5XHf0fqwWvKXmOFpgMW5B2SqLcgnSqapSMro7zqPMI_jV8UCzmNFWwvmAYTfB9RWwwx2SKTfBmQCBaxYQvtv8CZLG7Hval7ZmUu4kWuhLBU1NsmSNGUYahbjcaAJgUW8s8hpVlZZHvDArKP_Hg6J-m0dneDexrrId2eLA95CaVWJ9FdruahwLi2eSs0VGK2sqweSkciRZ0GiIRVLZcDoPZ3nUTH5wqJtMVKOZFJIz9UwJrT_Zxr3j8EgmROqxlL3PbuqOXlxvSPwl0DaCOOgpLpPQRrFjyNlW1SxW1udj6Cf9BygSI43jwnasQlwi8vLzj_0fht2QbATmk-TWvziJ1oEanvTO9tgo4zZfqb3jlkdJc3kKLUB1YWu5h7BuQnrmU_VkdoCj-6IcIf4OnZ0BMiOaak0WbK9dTkhxDWs9vA5hgx6acoXtn3QO15ym4A2fR5lMv1sMurUXjuqOdAGefliFF_3ZofXO1xWp4L8259S0g2RIYazxNPOOWLpqnjfFBzO9_28ORE_zJAw-sR1MRmHmaJ3uci6Z6CbKNHHNNLWmgKvueoO9JPvbeV2ksmnZcZo0DXNobUjbKLEF1osDRBCaMBlMwkiqJLCrkaHEESCxWJzRlYOPm-puUad1j7DaXMQEGY1VrmXXeQuW3SJMAkU58OSLWcfJXEh3Qg4VJxzC_qK1ebHpCDYOKAlAh2wwod7IJrJsT4YKtSoQj3-2xhQQRPYtdIfFGs6Z5-XAbiWV6B8FZD-rXlg4z83XOPz8Fysnq6pp-VXsYkv93QzWRXSZEdWMzSWkykf0wupIe4CdSVq_ByOe8kaMClhRxHwVM-qziwI-_4Ss_zJSV7ZUF6JfGaTYzPKt5J9jqzmP9-FqJyX-H0B-r4Jf2mtPakFSGO4fn-0zNjPmc7m59rJ6N0-5NVcN2SzhHcZrRHSDb__TQz9NBsuBE1RiNhkONk8AyqV4AxrW6MAkdM_PCAklw66QBzHqXedAhTGhGUb12degNw8hXv2e2RzUAsA5LU_quao1hixzkLyYIrXeJT-qhJtsg39lTF3-yZWfcrPpIoPLy5LW4_nRHV5Jo0QQCXSHw_-3tFLjjmFOLVX1bJEX1NAhY6hWV0Ci5YqWjnHrGyWLUMNglx_EsCv3QMDznmnSGlpQzc7tnHcQiRj4PGU6E-N9fDa0iIT4UjuJHXO4SlvTkJ4qDLENKSeCdGako-wSP856Sv6D2OQkEBJkh-fA5D1VHQ8flDFxvnLvOloU4E7moX1GibyzBqG5JBEpUu54crgWvIiShLkul28p4OSOdMEbKeWsqqI-fh4iA1uy4vLJuQ4LtaPV5T6yLhEm-ADzPcJiMbiRgxu0Xxz5pA7JUSCYm9w0GgCKGPScghgjy1JFu4FJ_FmV9fGPLy0nauHqaxpu67uy_qYDmm9r25mJrXmxOMiywW_pylm2Vj2GZw4JFGpLn05NFOsEqE6XvpW1F5R1bPUYTka9QpZ6cROKS1ZZvmdHfX4MK0aS-rWcc-uiXFK2M3ugqvqg8qh148NzfgyvyuYN81HbLQHH2uEdX1dUKsVI","summary":[]},"output_index":0,"sequence_number":3}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"fc_09365a692b7f9e41016aec0b0d4bb087d2b6d95afb4385d6a9","type":"function_call","status":"in_progress","arguments":"","call_id":"call_ABlbzgFUG30hFqoFtg5yqoXw","name":"get_weather"},"output_index":1,"sequence_number":4}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","delta":"{\"city\":\"Paris\"}","item_id":"fc_09365a692b7f9e41016aec0b0d4bb087d2b6d95afb4385d6a9","obfuscation":"","output_index":1,"sequence_number":5}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","arguments":"{\"city\":\"Paris\"}","item_id":"fc_09365a692b7f9e41016aec0b0d4bb087d2b6d95afb4385d6a9","output_index":1,"sequence_number":6}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"fc_09365a692b7f9e41016aec0b0d4bb087d2b6d95afb4385d6a9","type":"function_call","status":"completed","arguments":"{\"city\":\"Paris\"}","call_id":"call_ABlbzgFUG30hFqoFtg5yqoXw","name":"get_weather"},"output_index":1,"sequence_number":7}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"fc_09365a692b7f9e41016aec0b0d4bc087d296d3d0549731fc7d","type":"function_call","status":"in_progress","arguments":"","call_id":"call_F3njruPMqewUDDCyQBm1igzu","name":"get_weather"},"output_index":2,"sequence_number":8}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","delta":"{\"city\":\"Lyon\"}","item_id":"fc_09365a692b7f9e41016aec0b0d4bc087d296d3d0549731fc7d","obfuscation":"c","output_index":2,"sequence_number":9}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","arguments":"{\"city\":\"Lyon\"}","item_id":"fc_09365a692b7f9e41016aec0b0d4bc087d296d3d0549731fc7d","output_index":2,"sequence_number":10}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"fc_09365a692b7f9e41016aec0b0d4bc087d296d3d0549731fc7d","type":"function_call","status":"completed","arguments":"{\"city\":\"Lyon\"}","call_id":"call_F3njruPMqewUDDCyQBm1igzu","name":"get_weather"},"output_index":2,"sequence_number":11}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"fc_09365a692b7f9e41016aec0b0d4bc887d2ab3ec3a858236f2d","type":"function_call","status":"in_progress","arguments":"","call_id":"call_y9S5QQmmKqI0miSQ5iB99iOw","name":"get_weather"},"output_index":3,"sequence_number":12}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","delta":"{\"city\":\"Nice\"}","item_id":"fc_09365a692b7f9e41016aec0b0d4bc887d2ab3ec3a858236f2d","obfuscation":"c","output_index":3,"sequence_number":13}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","arguments":"{\"city\":\"Nice\"}","item_id":"fc_09365a692b7f9e41016aec0b0d4bc887d2ab3ec3a858236f2d","output_index":3,"sequence_number":14}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"fc_09365a692b7f9e41016aec0b0d4bc887d2ab3ec3a858236f2d","type":"function_call","status":"completed","arguments":"{\"city\":\"Nice\"}","call_id":"call_y9S5QQmmKqI0miSQ5iB99iOw","name":"get_weather"},"output_index":3,"sequence_number":15}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_09365a692b7f9e41016aec0b0bb53487d2b6638f5188d3f563","object":"response","created_at":1793854219,"status":"completed","access_programs":null,"background":false,"completed_at":1793854221,"error":null,"frequency_penalty":0.0,"image":{"context_compression":"disabled"},"incomplete_details":null,"instructions":null,"max_output_tokens":2000,"max_tool_calls":null,"model":"gpt-5.4-mini-2026-03-17","moderation":null,"output":[{"id":"rs_09365a692b7f9e41016aec0b0cb2e087d2bf859493e85fd7f9","type":"reasoning","content":[],"encrypted_content":"gAAAAABq7AsNs_nuRY7HvNy5ERJNRLr2MR3sHutuumm6P2CPWFDwZ8_nVnJWID59sZjFJliA4vy_Tk6RQz1N8s4Zu2kpWd5QnGqenDjX7j9IiV0zKZo2FQsBNgm9NyaeoJiDrApb7l42_6yMmUmZpBxxCVPiNVAEcifCE6b3AS7W9bkUdkRklelwN6AgJdWa171gThpAydo_9zhKOJgl-5niitohT7wVNmOTsnQYW22RqHrSuKJCC4kVvKp80teC6q_arv9aZGHkvVbAeov9YIJAdPlNy3LBiT5fPTOF69wl_AcGr1dF647G6SDSlabr7LWV8yUeq-rf2Id8RFzoj2fTzN1hYGR8TavduniPGz_muwz2wkcQm4x_P2U7msKwIyLUYhVFLpgkS37CMTGWQAH8_e8zEcD2xJ3bFt0wIbLPVbY1af-sRUjY1t-5K5CFB4R2axRh9TKV5HbarVEfpsyPTxiWSjyWo15KOzHboaI80FuU3OzkYFITkDfYIkS08S0s5MypZMvUNXL-8tm51XGmK52CWUgHSKiuwMZze890LCsflfWc6O_doiCjyNqkcZiM8kmBzkZy5rKLiXkFmtswV5PeyUcT-lkR6eEEqRj2euSojM9HaOE6vRbRUhr5RAJ8gXVHVwMp0hxTUXverEn0ga8NtvioZBroHeyEsYB_ueIqy2Sm_eFbGByzizFaKS_WQvlhZVuohbSQmbyG-ipk8sPAJVh2i6SheeHSEGRIDoWKNKH4D5hXyvSIMdWP5CaScvC9YUdNcdYKpAei6DQpwZs7Ny0IkNo1i__DxZDNrPAzsp14LznhNleCS48xjdNjQTQvEulRm-feitc0Tn1bRFNH_x9HLT5dDjp9lusEL9IkvRe4xcyxVkcfQqnDR-tfs_Axc3wbE5Tak4jaY39dyReescXpVEJeym-aRwo_sSSA6MMaiSDwIk659icduwDd_5p64H2yrnOkfgrv2W1FsGSoRK9vAlOHQzUQQ7JGebbo4uv850q3wlnNR_DttlUR7svG7jVVU4uBxhRQGraVUHkPibv0JBrXzITkJiK7uOyKqEEcIWMJwdvXwmRZffoSk6Bb4dUOk2szGZMsa9EZBTAaVK2rt2lzuwipEqtHgowmmjNjIOG1RoYK4nA5rU3K7Yhg4yEhSGzkizVSSqS_tk_CwY9AseoemkM653tfgpkK6CDmf8keFmvKl63Gvg_hzg69KQaoLLpsVfNc_JQh17YGbgI1dzjnprJg55TGCIFf49yt1jgQuNNCUQVS8K23dnIupfpjDjmicJ9F_Bm-J-OEULsDRHmnt-dCvn0IQ_OMarlaiCpX20vMTVFJWecs1nKQzLCQdhvvrSf-USxGxFw7A8pxzU4sb0AzJO-lt8nXVqPz9zlIbvZsEvXVaJ45cMkmp0wT0LXRMTHUIq7we-IrDtWmM4-9TBJGLPrWmyPBvLWshddUFwdynYDqjp2lPUkd_U804QQBD2De4kSLZ-WOsS35tkXwQ3UmlkjoOCKzxTdHusKowAjbZr3XnMV8hRTw0MTU_vZlWkoNe1KKs50kKj8Du7CvcO9wpJpPHvkOr2o1yDStLd5IXxMnarZthreO9icgjTMpJzmIJqBUvD6IaxpyxoMXofkxH_J27_U6YXTFO-rFCINID6UGHpxHl0ef0ZvpXrWfiUndlnvNcTF2Rmt1xcr-HavFRelo6sjTpWvPiAhN7LLyrjBhTEMDjnjuiTQzH27Fz_2hvWBUPi6bDQNasIhxQj1sOTJ_cQcDkhHKP1cA8le3hpDxZMiIwVn8LwkkmblHJNtRlAfu7L2ZBp0uzNVqrPUHdMN9NQhMQ958hvY51_d799YI3HAJQ_bvmCU5l7ip-QymGKOyT4R-uY4SVNbFWRkLQwMw6vOK5o1rfd9PzcrtzhjHaxLLIAi9hqD3rk7cr0_BKHovJNt4Ht9aGhPIqzagOe1SnV3AnO-rktSaZUq98mYs4GdWABJ_YusTYhfj4fLCdmrsBB6JbZgOMy1nd7zZskvLIAI4rj60wb3KJg31yXUGxJR7qZnyd4VjnA0FCAIXmKGWZ2D9wHYNDwxzZICHmZ7DY7B40rIT4CYnfKqzPqJeaW-EJTwi7xHnZM-otD-Pe9k5m3ifyat7JvgFG8tdmaSt2EDEOWgXuY0jtdT6NTzlFbZo5dp2Ni1lHjUhCynAHAVXj5JONObaFNaCNxeUd0vc8V3sMrrfEX7t7timUJlKHSQSuG5CbscVFsG3tOGW-gftaEhh9QrLBk4iGiibnlcEwCHvpvCNHSfng3kUP91h7dFt2Sw5XoLY7WTEwEp5BR4LHYRuiydu9AWdEwpjf5o9W0LBVuImj2Vv9n8Mhd8NA2ncqT_i00O6S35G7PbYLrOYY5YdgmAXEyItmHLjNP-b4Vyqrvqz8GSKr5Gy7HFJFm0ZlZuvHEzMhUScGq2CY17u4s-oi13NSc3cZVTFoQQz6K9vKaEHcqdWAF-DH-mQRDA3xnCRd_vbui233zpWoVJvde7P_C3z9JD7I0ELjlMDX2TvAdXJHSflLMWOlhurpCtIzZQ5xKCtvpyUKTAZQF0SdFzg042PJ-pEdYM77tOKhBfvR25Emyh-8DB9zyoqlKSoEqRxWXj_kVuaxleogBNn7MvtGYbi0cBtD-hEwUqXZH9NO8AUlUlp6C5N4yT6-71uZydryNl3YZBc1WvUXaC5I9QeiocCeFvctApq5hVBe6tYhggj3y_YuyuvrJkSC11rQm1b4gLfL5OmJTfEgVrEYXIrdysy-XJGbI5Z5p0c3oqzLiGI4HO5MUQhtWt4EkgDhMVNJe5FkAG5tKz-LvQn2J0N633fRjhNDybdnICiHlgMUi34Pv9jqcNphTQj63nZO3PSKM7i","summary":[]},{"id":"fc_09365a692b7f9e41016aec0b0d4bb087d2b6d95afb4385d6a9","type":"function_call","status":"completed","arguments":"{\"city\":\"Paris\"}","call_id":"call_ABlbzgFUG30hFqoFtg5yqoXw","name":"get_weather"},{"id":"fc_09365a692b7f9e41016aec0b0d4bc087d296d3d0549731fc7d","type":"function_call","status":"completed","arguments":"{\"city\":\"Lyon\"}","call_id":"call_F3njruPMqewUDDCyQBm1igzu","name":"get_weather"},{"id":"fc_09365a692b7f9e41016aec0b0d4bc887d2ab3ec3a858236f2d","type":"function_call","status":"completed","arguments":"{\"city\":\"Nice\"}","call_id":"call_y9S5QQmmKqI0miSQ5iB99iOw","name":"get_weather"}],"parallel_tool_calls":true,"presence_penalty":0.0,"previous_response_id":null,"prompt_cache_key":null,"prompt_cache_retention":"24h","reasoning":{"context":"current_turn","effort":"medium","mode":"standard","summary":null},"safety_identifier":null,"service_tier":"default","store":false,"temperature":1.0,"text":{"format":{"type":"text"},"verbosity":"medium"},"tool_choice":"auto","tool_usage":{"image_gen":{"input_tokens":0,"input_tokens_details":{"image_tokens":0,"text_tokens":0},"output_tokens":0,"output_tokens_details":{"image_tokens":0,"text_tokens":0},"total_tokens":0},"web_search":{"num_requests":0}},"tools":[{"type":"function","description":"Current weather for a city.","name":"get_weather","output_schema":null,"parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]},"strict":false}],"top_logprobs":0,"top_p":0.98,"truncation":"disabled","usage":{"input_tokens":72,"input_tokens_details":{"cache_write_tokens":0,"cached_tokens":0},"output_tokens":175,"output_tokens_details":{"reasoning_tokens":110},"total_tokens":247},"user":null,"metadata":{}},"sequence_number":16}

"##;

/// A real stream cut by a 16-token output limit, ending in `response.incomplete`.
pub(super) const INCOMPLETE_STREAM: &str = r##"event: response.created
data: {"type":"response.created","response":{"id":"resp_0299ac42cd382c6c016aec0b0eb42c87d296b078f78d5b3b31","object":"response","created_at":1793854222,"status":"in_progress","access_programs":null,"background":false,"completed_at":null,"error":null,"frequency_penalty":0.0,"image":{"context_compression":"disabled"},"incomplete_details":null,"instructions":null,"max_output_tokens":16,"max_tool_calls":null,"model":"gpt-5.4-mini-2026-03-17","moderation":null,"output":[],"parallel_tool_calls":true,"presence_penalty":0.0,"previous_response_id":null,"prompt_cache_key":null,"prompt_cache_retention":"24h","reasoning":{"context":"current_turn","effort":"none","mode":"standard","summary":null},"safety_identifier":null,"service_tier":"auto","store":false,"temperature":1.0,"text":{"format":{"type":"text"},"verbosity":"medium"},"tool_choice":"auto","tool_usage":{"image_gen":{"input_tokens":0,"input_tokens_details":{"image_tokens":0,"text_tokens":0},"output_tokens":0,"output_tokens_details":{"image_tokens":0,"text_tokens":0},"total_tokens":0},"web_search":{"num_requests":0}},"tools":[],"top_logprobs":0,"top_p":0.98,"truncation":"disabled","usage":null,"user":null,"metadata":{}},"sequence_number":0}

event: response.in_progress
data: {"type":"response.in_progress","response":{"id":"resp_0299ac42cd382c6c016aec0b0eb42c87d296b078f78d5b3b31","object":"response","created_at":1793854222,"status":"in_progress","access_programs":null,"background":false,"completed_at":null,"error":null,"frequency_penalty":0.0,"image":{"context_compression":"disabled"},"incomplete_details":null,"instructions":null,"max_output_tokens":16,"max_tool_calls":null,"model":"gpt-5.4-mini-2026-03-17","moderation":null,"output":[],"parallel_tool_calls":true,"presence_penalty":0.0,"previous_response_id":null,"prompt_cache_key":null,"prompt_cache_retention":"24h","reasoning":{"context":"current_turn","effort":"none","mode":"standard","summary":null},"safety_identifier":null,"service_tier":"auto","store":false,"temperature":1.0,"text":{"format":{"type":"text"},"verbosity":"medium"},"tool_choice":"auto","tool_usage":{"image_gen":{"input_tokens":0,"input_tokens_details":{"image_tokens":0,"text_tokens":0},"output_tokens":0,"output_tokens_details":{"image_tokens":0,"text_tokens":0},"total_tokens":0},"web_search":{"num_requests":0}},"tools":[],"top_logprobs":0,"top_p":0.98,"truncation":"disabled","usage":null,"user":null,"metadata":{}},"sequence_number":1}

event: response.output_item.added
data: {"type":"response.output_item.added","item":{"id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","type":"message","status":"in_progress","content":[],"phase":"final_answer","role":"assistant"},"output_index":0,"sequence_number":2}

event: response.content_part.added
data: {"type":"response.content_part.added","content_index":0,"item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","output_index":0,"part":{"type":"output_text","annotations":[],"logprobs":[],"text":""},"sequence_number":3}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":"The","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"zdJywN1BGiGvA","output_index":0,"sequence_number":4}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" sky","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"dqj3fiN7TO74","output_index":0,"sequence_number":5}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" looks","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"EtpdecJAXf","output_index":0,"sequence_number":6}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" blue","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"mV3FZbVFn4k","output_index":0,"sequence_number":7}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" mainly","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"5fw2txO1q","output_index":0,"sequence_number":8}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" because","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"Ywk0ePCe","output_index":0,"sequence_number":9}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" of","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"1CChR3YkGdEJo","output_index":0,"sequence_number":10}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" **","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"DiWFA1KLAUqWw","output_index":0,"sequence_number":11}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":"Ray","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"vzpE2bAod87S6","output_index":0,"sequence_number":12}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":"leigh","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"hzAu1GcSI9m","output_index":0,"sequence_number":13}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":" scattering","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"JTruS","output_index":0,"sequence_number":14}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":"**","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"8oxAcz6CeQwsLX","output_index":0,"sequence_number":15}

event: response.output_text.delta
data: {"type":"response.output_text.delta","content_index":0,"delta":".\n\n","item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"obfuscation":"LBCQRclqOlteG","output_index":0,"sequence_number":16}

event: response.output_text.done
data: {"type":"response.output_text.done","content_index":0,"item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","logprobs":[],"output_index":0,"sequence_number":17,"text":"The sky looks blue mainly because of **Rayleigh scattering**.\n\n"}

event: response.content_part.done
data: {"type":"response.content_part.done","content_index":0,"item_id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","output_index":0,"part":{"type":"output_text","annotations":[],"logprobs":[],"text":"The sky looks blue mainly because of **Rayleigh scattering**.\n\n"},"sequence_number":18}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","type":"message","status":"incomplete","content":[{"type":"output_text","annotations":[],"logprobs":[],"text":"The sky looks blue mainly because of **Rayleigh scattering**.\n\n"}],"phase":"final_answer","role":"assistant"},"output_index":0,"sequence_number":19}

event: response.incomplete
data: {"type":"response.incomplete","response":{"id":"resp_0299ac42cd382c6c016aec0b0eb42c87d296b078f78d5b3b31","object":"response","created_at":1793854222,"status":"incomplete","access_programs":null,"background":false,"completed_at":null,"error":null,"frequency_penalty":0.0,"image":{"context_compression":"disabled"},"incomplete_details":{"reason":"max_output_tokens"},"instructions":null,"max_output_tokens":16,"max_tool_calls":null,"model":"gpt-5.4-mini-2026-03-17","moderation":null,"output":[{"id":"msg_0299ac42cd382c6c016aec0b0f321c87d2be770edf0e8f69a4","type":"message","status":"incomplete","content":[{"type":"output_text","annotations":[],"logprobs":[],"text":"The sky looks blue mainly because of **Rayleigh scattering**.\n\n"}],"phase":"final_answer","role":"assistant"}],"parallel_tool_calls":true,"presence_penalty":0.0,"previous_response_id":null,"prompt_cache_key":null,"prompt_cache_retention":"24h","reasoning":{"context":"current_turn","effort":"none","mode":"standard","summary":null},"safety_identifier":null,"service_tier":"default","store":false,"temperature":1.0,"text":{"format":{"type":"text"},"verbosity":"medium"},"tool_choice":"auto","tool_usage":{"image_gen":{"input_tokens":0,"input_tokens_details":{"image_tokens":0,"text_tokens":0},"output_tokens":0,"output_tokens_details":{"image_tokens":0,"text_tokens":0},"total_tokens":0},"web_search":{"num_requests":0}},"tools":[],"top_logprobs":0,"top_p":0.98,"truncation":"disabled","usage":{"input_tokens":13,"input_tokens_details":{"cache_write_tokens":0,"cached_tokens":0},"output_tokens":16,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":29},"user":null,"metadata":{}},"sequence_number":20}

"##;
