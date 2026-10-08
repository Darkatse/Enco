#![expect(
    clippy::unwrap_used,
    reason = "fixture helpers fail the test by panicking"
)]
#[path = "../src/client.rs"]
mod client;
#[path = "../src/protocol.rs"]
mod protocol;
mod support;

use enco_core::*;
use enco_kernel::{Answer, Inspection, Label, Question, QuestionKind, Runtime};
use enco_wasm::WasmRuntime;
use protocol::Command;
use serde_json::json;
use support::*;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

const DRINK: &str = "主人喜欢喝无糖红茶。";
const EDITOR: &str = "主人使用 Neovim 编辑器。";
const QUERY: &str = "我想喝点东西，推荐什么？";

#[tokio::test]
async fn decision_plugin_maps_distributions_and_reports_bad_responses() {
    let server = MockServer::start().await;
    Mock::given(path("/systemone"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "answers": {
                "q1": { "type": "score", "probabilities": { "1": 0.7, "0": 0.3 } },
                "q0": { "type": "choice", "probabilities": { "a": 0.2, "z": 0.8 } }
            }})),
        )
        .mount(&server)
        .await;
    let loaded = WasmRuntime::new()
        .unwrap()
        .load(include_bytes!(env!("ENCO_FACTORY_TYPESAFE")), &json!({}))
        .await
        .unwrap();
    let decision = loaded.decision.unwrap();
    let settings = ProviderSettings {
        base_url: server.uri(),
        model: "mock".into(),
        api_key_env: None,
        options: json!({}),
    };
    let labels = vec![
        Label {
            name: "z".into(),
            description: None,
        },
        Label {
            name: "a".into(),
            description: Some("second".into()),
        },
    ];
    let questions = vec![
        Question {
            instructions: "Choice".into(),
            kind: QuestionKind::Choice(labels.clone()),
        },
        Question {
            instructions: "Score".into(),
            kind: QuestionKind::Score(labels),
        },
    ];
    let answers = decision
        .decide(&settings, None, "evidence".into(), questions)
        .await
        .unwrap();
    assert!(matches!(&answers[0], Answer::Choice(p) if p == &[0.8, 0.2]));
    assert!(matches!(&answers[1], Answer::Score(p) if p == &[0.3, 0.7]));

    server.reset().await;
    Mock::given(path("/systemone"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "answers": {
                "q0": { "type": "noul", "noul": 1.2 }
            }})),
        )
        .mount(&server)
        .await;
    let error = decision
        .decide(
            &settings,
            None,
            "evidence".into(),
            vec![Question {
                instructions: "Predicate".into(),
                kind: QuestionKind::Predicate,
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, code::PROVIDER_BAD_RESPONSE);
}

async fn ask(daemon: &Daemon, session: &str, text: &str) -> Inspection {
    let mut client = daemon.connect().await;
    client
        .request(Command::Subscribe {
            session: session.into(),
        })
        .await
        .unwrap();
    client
        .send(Command::Send {
            session: session.into(),
            text: text.into(),
            event_id: EventId::new(),
        })
        .await
        .unwrap();
    let entries = finish(&mut client).await;
    assert!(matches!(
        entries.last().unwrap().body,
        EntryBody::RunEnded {
            end: RunEnd::Completed,
            ..
        }
    ));
    serde_json::from_value(
        client
            .request(Command::Inspect {
                session: session.into(),
                attempt_id: None,
            })
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn relevance_filters_new_recall_and_service_failure_keeps_it() {
    let server = MockServer::start().await;
    embeddings(&server).await;
    Mock::given(path("/chat/completions"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let messages = body["messages"].as_array().unwrap();
            if messages.last().unwrap()["content"] == "seed" {
                let calls: Vec<_> = [DRINK, EDITOR]
                    .into_iter()
                    .enumerate()
                    .map(|(i, text)| {
                        tool_message(
                            &format!("save-{i}"),
                            "memory_save",
                            json!({ "text": text, "pinned": false }),
                        )["tool_calls"][0]
                            .clone()
                    })
                    .collect();
                response(
                    json!({ "role": "assistant", "tool_calls": calls }),
                    "tool_calls",
                )
            } else {
                response(json!({ "role": "assistant", "content": "done" }), "stop")
            }
        })
        .mount(&server)
        .await;
    let base_url = server.uri();
    let config = format!(
        "{}\n[decision]\nplugin = 'typesafe'\nbase_url = {base_url:?}\nmodel = 'mock'\n",
        Daemon::config(&server, "openai-compatible")
    );
    let mut daemon = Daemon::start_config(&config).await;
    // Seed in a separate Session so tool arguments do not become the query's history.
    ask(&daemon, "seed", "seed").await;
    let decision_response = Mock::given(path("/systemone"))
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let answers: serde_json::Map<_, _> = body["questions"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, question)| {
                    let p = if question["instructions"].as_str().unwrap().contains(DRINK) {
                        0.9
                    } else {
                        0.1
                    };
                    (key.clone(), json!({ "type": "noul", "noul": p }))
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({ "answers": answers }))
        })
        .mount_as_scoped(&server)
        .await;
    let filtered = ask(&daemon, "filtered", QUERY).await;
    let text = filtered
        .request
        .messages
        .iter()
        .map(Message::joined_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains(DRINK));
    assert!(!text.contains(EDITOR));
    assert!(
        filtered
            .plan
            .omitted
            .iter()
            .any(|o| o.reason.starts_with("judged irrelevant to the input"))
    );

    drop(decision_response);
    Mock::given(path("/systemone"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    let fallback = ask(&daemon, "fallback", QUERY).await;
    let text = fallback
        .request
        .messages
        .iter()
        .map(Message::joined_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains(DRINK) && text.contains(EDITOR));
    assert!(
        fallback
            .plan
            .omitted
            .iter()
            .any(|o| o.source == "memory:relevance" && o.reason.contains("provider.server"))
    );
    daemon.stop().await;
}
