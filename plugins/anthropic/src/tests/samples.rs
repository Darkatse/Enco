//! Real Messages streams from `claude-haiku-5-5`, kept verbatim as the tests' samples.

/// Adaptive thinking at high effort, its text omitted, then a call to `get_weather`.
pub(super) const THINKING_CALL: &str = r##"event: message_start
data: {"type":"message_start","message":{"model":"claude-haiku-5-5","id":"msg_011CgjBHbajud1Mkn48yJ8dX","type":"message","role":"assistant","content":[],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":406,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0},"output_tokens":6,"service_tier":"standard","inference_geo":"global","speed":"standard"},"diagnostics":null}   }

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}          }

event: ping
data: {"type": "ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}        }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"CAQSsgsKEAgTGAI4AUIIdGhpbmtpbmcSDPrrOJ0NVGJuSpixpxoM7rV35MzERPUJ6P5TIjDEfVEdeXvCW+QaMKopO9Uyz3abau7yQS4ZA9VttRLkSH2NWCpkDr9v94cXVPvQ4J8qzwow9iOD/8nTGolu2XvJ8qOVVXGsHE2+EZ7aXQl8n8KglhYNyWdodS3HerX7aB8wTnfCBwKfs+X8wGKYnZ/3JD9cyX/X6IHtAH84jEjziFG6LYkgFDyJ21ydcWkZL/gUWx3i4iCXemOWpy+dfUamokkLzLwH3qtjP4eqTgyCfm2eDLJ2bsm3L9xMPq20nvawEJDIVH13ifK+D7+zKXDuH3PtXYCGuFZeJ1eiTiAChMNBTfcHfpYyF11cObVgDnEvDbj9S25ziSjEpLOvuNr7Mdd2Uqb6vt1uWZSzmb8JaqCTD9Hc5cOBdEq+iJmrOTDRsZpXyqbWlFEtSP2Nzvz1ZRsG3DNzWaR/E3guqYWvVGxBkRVzZ7gna2nfyJ+5RTma43mlCcecR9u9cUon3FD3RWAT3RMFQRK5mhJpfxEeCewfk+VvZQb6Au658R6R6wrr2riIvIGvloBvW4CGexcVwO4XZdOaTTv8+hBOTDQ1oT7Awyjdu83Xwth79Ep73628dIEylneu3M3iPAFMoEmt0TI4V4ECnqL3wYN/ut+xNzk702Op9At/Dr7J4c3vA9DlncbBf6D65/w2aFD4ZjsXWlW3zY35DonWU86h4PqWHN7pffXBFHA8jjLoZv1VgVIn8JMtdIDyKsY+WJN7YFpD1aqYRftInv4G72ecRfAs/5/yH1EVGabIP5iLjw66hcgfGYGWHcs8r1UwOpjk6lV6QSXjn07y2Vwm2Dishlt2kzZj31m7rxb3C38+DXHPit9WBsY4JioX6a2emQ+8CTcKkbmGSX1Uu2Ba23bA+iekMUOl8gTXDsMGfhGsOHvbDP/gbsQ/zIOTbiY9tWQkKi8/PHtR3vvjUDSJBX4dj/bAnSY2qeJwR7MzvZVTpLrs4HWsTZl87l4XGBqiS2B7R1YdpElIWcd2PPtG+Eos/+IbkC5EOX4d074Y1uHzZDbHf3dHW2SmHwOs1pPyEwa7Urh1EIWzZzZPuSTQWe4OkEeyxmQgnX4ZwFmshRvi1tVCF+z8gsXy5i1JJCh8UlR8hX7X9bzEet9KU8Jdn1LrpPU6df7WFaJlC/C7Up5OuQEeymaTLY3fF9h42hJbWwW/BJ2IxlP2h6SuB8aMidlnU1G1Ci01gOCkaVyL56x7ImbJ9ehlXEoRETL7LhUJDIxAqShA7JMZQbxXhRGNTCUg7+tL1WU1WsIt2avhXZ4z4FKvzYM/Hq/TjcaJwLA3AWB7Tt+qUrGxGA6QmiH0Rvwifr8fe0sLfVQUgbKdec0DncSP99EKFQQz+6Oj4ASaswCbbzM1N28f7mLat0P5K3PV9Gw5scNpvzyE8BSzngH/iWf0dpQ8qswURiGHhvsZsqvWWJY4kt6QZblPFMwuNC0R1LiDB6jvohrTwJtWek3ZvgNjr+s+oPi3Q0lLnBbMrS32FaHIdTXlWBLyd0XIcJcrmN+QOp9Isq/xe3wcVTOKPeARdb3ezhTVFYp05tJOsVjxEe0zKcr8a6ykDU3o8prCVe9dz6Eb3CkOI441sujcR0lNM5hucZkZ1HJXQfs5X8woFyHKdcPFO4S4dfiYHzs8S/xdroMORkVB6B1UbJrAWDmGCB8CMBDIf2gC7DXF+ULsVH2MEyh9T3rPbt6ohXa+eMII2BONQ1T/D+vMQRBw5FAucZVL6sqAHLppFm1twKkwfSWCLh/ggcg11+2LjHIGkLqhc+F1hON1hEONn1MCIllo2eDiU22lBHmywmv7s6ufWOVn6YnIReojpEeF/mWFuO7YIqWEIuPTajXRolThAk4TEtARF2mlhve+ViBjya34lg94docYAQ=="}       }

event: content_block_stop
data: {"type":"content_block_stop","index":0         }

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_017GSxXn5j52bDCWB5mTXMoc","name":"get_weather","input":{},"caller":{"type":"direct"}}             }

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":""}    }

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\""}   }

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":": \"Nice"}               }

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1      }

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null,"stop_details":null,"container":null},"usage":{"input_tokens":406,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":331,"output_tokens_details":{"thinking_tokens":280}}  }

event: message_stop
data: {"type":"message_stop"    }

"##;

/// A text reply.
pub(super) const TEXT: &str = r##"event: message_start
data: {"type":"message_start","message":{"model":"claude-haiku-5-5","id":"msg_011CgjBHmGeCmFep7frd1fJQ","type":"message","role":"assistant","content":[],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":16,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0},"output_tokens":4,"service_tier":"standard","inference_geo":"global","speed":"standard"},"diagnostics":null}              }

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}      }

event: ping
data: {"type": "ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello there"}          }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":", fri"}      }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"end! How"}            }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" are you?"}       }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"\n\nW"}         }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ait, that's five words..."}             }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" let"}            }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" me count:"}               }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" Hello("}               }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"1) there(2) friend"}           }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"(3) How"}              }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"(4) are(5)"}     }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" you(6). That"}         }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"'s six, so here"}              }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"'s a corrected version:"}       }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"\n\nHello there, friend,"}         }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" good"}         }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" day"}       }

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"!"}    }

event: content_block_stop
data: {"type":"content_block_stop","index":0    }

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null,"stop_details":null,"container":null},"usage":{"input_tokens":16,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":80,"output_tokens_details":{"thinking_tokens":0}}          }

event: message_stop
data: {"type":"message_stop"              }

"##;

/// A forced call to `write_note` cut by a 40-token output limit: its block never stops.
pub(super) const CUT_CALL: &str = r##"event: message_start
data: {"type":"message_start","message":{"model":"claude-haiku-5-5","id":"msg_011CgjBHpnUkEFPBiCq4uRJb","type":"message","role":"assistant","content":[],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":503,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0},"output_tokens":16,"service_tier":"standard","inference_geo":"global","speed":"standard"},"diagnostics":null}    }

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_013SQH35NkLf8BKqBhVqpLtd","name":"write_note","input":{},"caller":{"type":"direct"}}     }

event: ping
data: {"type": "ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":""}  }

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null,"stop_details":null,"container":null},"usage":{"input_tokens":503,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":40,"output_tokens_details":{"thinking_tokens":0}}      }

event: message_stop
data: {"type":"message_stop"      }

"##;

/// A forced call to `get_time`, which takes no arguments.
pub(super) const NO_ARGUMENTS: &str = r##"event: message_start
data: {"type":"message_start","message":{"model":"claude-haiku-5-5","id":"msg_011CgjBHtk6o9ngkMvW79JgV","type":"message","role":"assistant","content":[],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":470,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0},"output_tokens":11,"service_tier":"standard","inference_geo":"global","speed":"standard"},"diagnostics":null}  }

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_018HWrNrhQs7QSQcwfuYgXYy","name":"get_time","input":{},"caller":{"type":"direct"}}               }

event: ping
data: {"type": "ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":""}               }

event: content_block_stop
data: {"type":"content_block_stop","index":0       }

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null,"stop_details":null,"container":null},"usage":{"input_tokens":470,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":11,"output_tokens_details":{"thinking_tokens":0}}    }

event: message_stop
data: {"type":"message_stop"          }

"##;
