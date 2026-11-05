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
